use chemifree_application::phase6_profiles::{
    CatalogIdentity, DatasetRecords, DecodeError, FileKind, Profile, Records, decode,
    decode_ascii_pair, decode_dataset, decode_decimal_reference, decode_f64_literal,
    decode_f64_with_unit, decode_latin1_pair, decode_u64,
};

#[test]
fn a_multi_file_dataset_is_admitted_only_as_one_complete_valid_set() {
    let a: String = (2..=109)
        .map(|z| format!("{{\"atomic_number_u32\":{z},\"source_rank_u32\":{z}}}\n"))
        .collect();
    let b = b"{\"atomic_number_u32\":7,\"source_rank_u32\":123}\n";
    let files = [
        ("normalized/element-order-a.jsonl", a.as_bytes()),
        ("normalized/element-order-b.jsonl", &b[..]),
    ];
    let DatasetRecords::NamingRanks { order_a, order_b } =
        decode_dataset(Profile::NamingRanks, &files).unwrap()
    else {
        panic!("wrong profile")
    };
    assert_eq!(order_a.len(), 108);
    assert_eq!((order_a[0].atomic_number, order_a[0].source_rank), (2, 2));
    assert_eq!((order_b[0].atomic_number, order_b[0].source_rank), (7, 123));

    assert!(decode_dataset(Profile::NamingRanks, &files[..1]).is_err());
    assert!(decode_dataset(Profile::NamingRanks, &[files[0], files[0]]).is_err());
    assert!(decode_dataset(Profile::NamingRanks, &[files[0], (files[1].0, b"bad\n")]).is_err());
    assert!(decode_dataset(Profile::NamingRanks, &[files[1], (files[0].0, b)]).is_err());
    assert!(decode_dataset(Profile::TypeElements, &files).is_err());
    let duplicate_a = a.replace(
        "{\"atomic_number_u32\":109,\"source_rank_u32\":109}",
        "{\"atomic_number_u32\":2,\"source_rank_u32\":109}",
    );
    assert!(
        decode_dataset(
            Profile::NamingRanks,
            &[(files[0].0, duplicate_a.as_bytes()), files[1]],
        )
        .is_err()
    );
}

#[test]
fn single_file_datasets_expose_only_their_selected_record_type() {
    let symbol = b"{\"atomic_number_u32\":6,\"symbol_hex\":\"58\"}\n";
    let DatasetRecords::ElementSymbols(symbols) = decode_dataset(
        Profile::ElementSymbols,
        &[("normalized/element-symbols.jsonl", symbol)],
    )
    .unwrap() else {
        panic!("wrong profile")
    };
    assert_eq!(
        (symbols[0].atomic_number, &symbols[0].symbol_bytes[..]),
        (6, &b"X"[..])
    );
    let type_row = b"{\"internal_type_u32\":17,\"atomic_number_u32\":6}\n";
    let DatasetRecords::TypeElements(types) = decode_dataset(
        Profile::TypeElements,
        &[("normalized/type-elements.jsonl", type_row)],
    )
    .unwrap() else {
        panic!("wrong profile")
    };
    assert_eq!((types[0].internal_type, types[0].atomic_number), (17, 6));
}

#[test]
fn metadata_identity_is_verified_against_actual_bytes() {
    let identity = CatalogIdentity {
        dataset_id: "fixture-dataset",
        dataset_version: "fixture-version",
        registry_sha256: "5090079365bb50b785f62bc81f68556d50dbf490b445a7859646238eab2ca003",
        profile_sha256: "d69809cef1dcd1d102da7c0865cd1a38c2551e2c68ed47044705c26901a465b1",
        manifest_file_sha256: "62e8e2227bdfba30eb4e6593b270fac4ccf19b18a86b0961fd1d413e4e5d1d30",
    };
    let manifest = b"synthetic manifest\n";
    let registry = b"synthetic registry\n";
    let profile = b"synthetic profile\n";
    assert_eq!(
        identity.check_file_bytes(manifest, registry, profile),
        Ok(())
    );
    assert_eq!(
        identity.check_file_bytes(b"changed", registry, profile),
        Err(DecodeError::CatalogMismatch)
    );
    assert_eq!(
        identity.check_file_bytes(manifest, b"changed", profile),
        Err(DecodeError::CatalogMismatch)
    );
    assert_eq!(
        identity.check_file_bytes(manifest, registry, b"changed"),
        Err(DecodeError::CatalogMismatch)
    );
    assert_eq!(
        Profile::ElementSymbols.verify_files(
            "fixture-dataset",
            "fixture-version",
            manifest,
            registry,
            profile
        ),
        Err(DecodeError::CatalogMismatch)
    );
    assert_eq!(
        Profile::ElementSymbols.verify_files(
            "legacy-caw-element-symbol-operands",
            "sha256-set-3a9716aacf119d6a927e1886ad7790fdf6b7b2d890856e2fe59830a35422f4a7",
            manifest,
            registry,
            profile,
        ),
        Err(DecodeError::CatalogMismatch),
        "matching labels do not establish the three file identities"
    );
}

#[test]
fn byte_and_text_witnesses_preserve_encoding_and_explicit_null() {
    assert_eq!(decode_latin1_pair("00ff41", "\0ÿA"), Ok(vec![0, 255, 65]));
    assert!(
        decode_latin1_pair("c3bf", "ÿ").is_err(),
        "UTF-8 bytes are not Latin-1 bytes"
    );
    assert!(decode_latin1_pair("ff", "Ÿ").is_err());
    assert_eq!(decode_ascii_pair(None, None), Ok(None));
    assert_eq!(decode_ascii_pair(Some(""), Some("")), Ok(Some(vec![])));
    assert_eq!(
        decode_ascii_pair(Some("4100"), Some("A\0")),
        Ok(Some(vec![65, 0]))
    );
    assert!(decode_ascii_pair(None, Some("")).is_err());
    assert!(decode_ascii_pair(Some("ff"), Some("ÿ")).is_err());
}

#[test]
fn authoritative_decimal_and_reference_bits_are_checked_and_preserved() {
    let value = decode_decimal_reference(br#"{"source_decimal":"+0.1","reference_f64":0.1,"reference_f64_bits_le":"9a9999999999b93f"}"#).unwrap();
    assert_eq!(value.source_decimal, "+0.1");
    assert_eq!(value.reference.bits, 0x3fb9_9999_9999_999a);
    let value = decode_decimal_reference(
        br#"{"source_decimal":"-.0","reference_f64":0,"reference_f64_bits_le":"0000000000000080"}"#,
    )
    .unwrap();
    assert_eq!(value.source_decimal, "-.0");
    assert!(value.reference.value().is_sign_negative());
    for invalid in [
        &br#"{"source_decimal":"NaN","reference_f64":0,"reference_f64_bits_le":"0000000000000000"}"#[..],
        br#"{"source_decimal":"1e","reference_f64":1,"reference_f64_bits_le":"000000000000f03f"}"#,
        br#"{"source_decimal":"0.2","reference_f64":0.1,"reference_f64_bits_le":"9a9999999999b93f"}"#,
        br#"{"source_decimal":"0.1","reference_f64":0.1,"reference_f64_bits_le":"000000000000f03f"}"#,
    ] { assert!(decode_decimal_reference(invalid).is_err()); }
}

#[test]
fn binary64_with_unit_retains_the_declared_unit_without_conversion() {
    let operand = decode_f64_with_unit(
        br#"{"value":1,"bits_le":"000000000000f03f","unit":"source-specific"}"#,
    )
    .unwrap();
    assert_eq!(operand.value.bits, 1.0_f64.to_bits());
    assert_eq!(operand.unit, "source-specific");
    for invalid in [
        &br#"{"value":1,"bits_le":"000000000000f03f"}"#[..],
        br#"{"value":1,"bits_le":"000000000000f03f","unit":null}"#,
        br#"{"value":2,"bits_le":"000000000000f03f","unit":"u"}"#,
        br#"{"value":1,"bits_le":"000000000000f03f","unit":"u","extra":0}"#,
    ] {
        assert!(decode_f64_with_unit(invalid).is_err());
    }
}

#[test]
fn full_u64_and_binary64_witnesses_keep_their_exact_bits() {
    assert_eq!(
        decode_u64(b"-0"),
        Ok(0),
        "JSON integer negative zero has value zero"
    );
    assert_eq!(decode_u64(b"18446744073709551615"), Ok(u64::MAX));
    assert_eq!(decode_u64(b"18446744073709551614"), Ok(u64::MAX - 1));
    for invalid in [
        &b"18446744073709551616"[..],
        b"1.0",
        b"1e0",
        b"-0.0",
        b"-0e0",
        b"-1",
        b"true",
    ] {
        assert!(decode_u64(invalid).is_err());
    }
    let tenth = decode_f64_literal(br#"{"value":0.1,"bits_le":"9a9999999999b93f"}"#).unwrap();
    assert_eq!(tenth.bits, 0x3fb9_9999_9999_999a);
    let zero = decode_f64_literal(br#"{"value":-0.0,"bits_le":"0000000000000080"}"#).unwrap();
    assert_eq!(zero.bits, (-0.0_f64).to_bits());
    assert!(zero.value().is_sign_negative());
    let numeric_zero = decode_f64_literal(br#"{"value":0,"bits_le":"0000000000000080"}"#).unwrap();
    assert_eq!(
        numeric_zero.bits,
        (-0.0_f64).to_bits(),
        "the witness retains the sign of a numerically equal zero"
    );
    let smallest = decode_f64_literal(br#"{"value":5e-324,"bits_le":"0100000000000000"}"#).unwrap();
    assert_eq!(smallest.bits, 1);
    for invalid in [
        &br#"{"value":1,"bits_le":"0000000000000040"}"#[..],
        br#"{"value":1,"bits_le":"000000000000f07f"}"#,
        br#"{"value":1,"bits_le":"000000000000F03F"}"#,
        br#"{"value":true,"bits_le":"000000000000f03f"}"#,
        br#"{"value":1,"bits_le":"000000000000f03f","extra":1}"#,
    ] {
        assert!(decode_f64_literal(invalid).is_err());
    }
}

#[test]
fn catalog_selects_exact_profile_identity_and_file_path() {
    let profile = Profile::parse("private-caw-element-symbols-v1").unwrap();
    assert_eq!(profile, Profile::ElementSymbols);
    assert_eq!(
        profile.file_kind("normalized/element-symbols.jsonl"),
        Some(FileKind::ElementSymbols)
    );
    assert_eq!(profile.file_kind("normalized/type-elements.jsonl"), None);
    let identity = CatalogIdentity {
        dataset_id: "legacy-caw-element-symbol-operands",
        dataset_version: "sha256-set-3a9716aacf119d6a927e1886ad7790fdf6b7b2d890856e2fe59830a35422f4a7",
        registry_sha256: "c1c012b6ce48eb7a3f60e23237a354a29cea1eb62dbc3c1a6123daba378917b9",
        profile_sha256: "d389e3b31461b708e62d897e8ff15ed5162cca4f3220d9f32ec946139bd899cd",
        manifest_file_sha256: "0e00bda8d9806053445e8f7b90e039bd6e6c818c988bc95849693a87e6411073",
    };
    assert_eq!(profile.check_identity(&identity), Ok(()));
    for other in [
        CatalogIdentity {
            dataset_id: "different",
            ..identity
        },
        CatalogIdentity {
            dataset_version: "different",
            ..identity
        },
        CatalogIdentity {
            registry_sha256: "different",
            ..identity
        },
        CatalogIdentity {
            profile_sha256: "different",
            ..identity
        },
        CatalogIdentity {
            manifest_file_sha256: "different",
            ..identity
        },
    ] {
        assert_eq!(
            profile.check_identity(&other),
            Err(DecodeError::CatalogMismatch)
        );
    }
    assert_eq!(
        profile.file_kind("normalized/../normalized/element-symbols.jsonl"),
        None
    );
    assert_eq!(
        Profile::parse("private-caw-type-elements-v1"),
        Some(Profile::TypeElements)
    );
    assert_eq!(
        Profile::parse("private-naming-element-ranks-v1"),
        Some(Profile::NamingRanks)
    );
    assert_eq!(
        Profile::NamingRanks.file_kind("normalized/element-order-a.jsonl"),
        Some(FileKind::ElementOrderA)
    );
    assert_eq!(
        Profile::NamingRanks.file_kind("normalized/element-order-b.jsonl"),
        Some(FileKind::ElementOrderB)
    );
    let type_identity = CatalogIdentity {
        dataset_id: "legacy-caw-internal-type-elements",
        dataset_version: "sha256-set-1b952eb5ed2812217f8deebaad52bbd69654ca9aa57e7b48a78e5d6e20092e91",
        registry_sha256: "3a6f93a097d6a8e9e43a4e7db083ef2007603318cc4717b81cf41455aabd245e",
        profile_sha256: "d66189d21c546cb578d3ea55886d4beb100dc9f0e4998f3af63fa2e41f6404da",
        manifest_file_sha256: "d5dde01383fc0b5c342e46085c8163c85c4035ba0614191513bc2ec741b3abfb",
    };
    let rank_identity = CatalogIdentity {
        dataset_id: "legacy-naming-element-priority-operands",
        dataset_version: "sha256-set-11cf9820814312415b52fedb6f9c9d3f8a5a8a7a515d32cd2ac7a2d285e018d8",
        registry_sha256: "237b961511043df4eac475d243b6cbe650d2f6ed08ae9feeb1afb75c0bbc544f",
        profile_sha256: "077826ec2c6971956504b61c8684c97323f62c06119f9ee9ea664ab8bf0def13",
        manifest_file_sha256: "f460a2ce2f4c68ce94a2c94d687c974d5dfe40c65c526127d5b9a5b3f94e4e03",
    };
    assert_eq!(Profile::TypeElements.check_identity(&type_identity), Ok(()));
    assert_eq!(Profile::NamingRanks.check_identity(&rank_identity), Ok(()));
    assert_eq!(
        Profile::NamingRanks.check_identity(&type_identity),
        Err(DecodeError::CatalogMismatch)
    );
}

#[test]
fn symbol_rows_preserve_order_repetition_and_exact_hex_bytes() {
    let Records::Symbols(zero) = decode(
        FileKind::ElementSymbols,
        b"{\"atomic_number_u32\":-0,\"symbol_hex\":\"00\"}\n",
    )
    .unwrap() else {
        panic!("wrong record type")
    };
    assert_eq!(zero[0].atomic_number, 0);
    let bytes = b"{\"atomic_number_u32\":4294967295,\"symbol_hex\":\"00ff\"}\n{\"atomic_number_u32\":4294967295,\"symbol_hex\":\"\"}\n";
    let Records::Symbols(rows) = decode(FileKind::ElementSymbols, bytes).unwrap() else {
        panic!("wrong record type")
    };
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].atomic_number, u32::MAX);
    assert_eq!(rows[0].symbol_bytes, [0, 255]);
    assert!(rows[1].symbol_bytes.is_empty());
    for invalid in [
        &b"{\"atomic_number_u32\":1,\"symbol_hex\":\"0A\"}\n"[..],
        b"{\"atomic_number_u32\":1,\"symbol_hex\":\"0\"}\n",
        b"{\"atomic_number_u32\":1.0,\"symbol_hex\":\"00\"}\n",
        b"{\"atomic_number_u32\":1e0,\"symbol_hex\":\"00\"}\n",
        b"{\"atomic_number_u32\":-1,\"symbol_hex\":\"00\"}\n",
        b"{\"atomic_number_u32\":null,\"symbol_hex\":\"00\"}\n",
        b"{\"atomic_number_u32\":4294967296,\"symbol_hex\":\"00\"}\n",
        b"{\"atomic_number_u32\":1,\"atomic_number_u32\":2,\"symbol_hex\":\"00\"}\n",
        b"{\"atomic_number_u32\":1,\"symbol_hex\":\"00\"}",
        b"\xef\xbb\xbf{\"atomic_number_u32\":1,\"symbol_hex\":\"00\"}\n",
        b"{\"atomic_number_u32\":1,\"symbol_hex\":\"00\"}\n\n",
    ] {
        assert!(decode(FileKind::ElementSymbols, invalid).is_err());
    }
}

#[test]
fn types_and_rank_contexts_remain_distinct_and_exact() {
    let types = b"{\"internal_type_u32\":4294967295,\"atomic_number_u32\":7}\n";
    let Records::Types(rows) = decode(FileKind::TypeElements, types).unwrap() else {
        panic!("wrong type")
    };
    assert_eq!(
        (rows[0].internal_type, rows[0].atomic_number),
        (u32::MAX, 7)
    );
    let rank = b"{\"atomic_number_u32\":7,\"source_rank_u32\":4294967295}\n";
    let Records::RanksA(a) = decode(FileKind::ElementOrderA, rank).unwrap() else {
        panic!("wrong context")
    };
    let Records::RanksB(b) = decode(FileKind::ElementOrderB, rank).unwrap() else {
        panic!("wrong context")
    };
    assert_eq!((a[0].atomic_number, a[0].source_rank), (7, u32::MAX));
    assert_eq!(b[0].source_rank, u32::MAX);
    assert!(
        decode(
            FileKind::ElementOrderA,
            b"{\"atomic_number_u32\":7,\"source_rank_u32\":1,\"directive\":1}\n"
        )
        .is_err()
    );
}
