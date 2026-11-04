//! V3000 codec core: reader, writer and refusals. Fixtures under `tests/data/ctfile-v3000/` are
//! hand-authored from the public CTfile description; expected values are written out here, not
//! computed by the code under test.

use chemifree_application::ctfile::{
    CT_REASON_FAMILIES, CtAtom, CtBond, CtBondStereo, CtCodec, CtCollection, CtCollectionKind,
    CtFormat, CtRadical, CtRecord, CtTab, ct_reason_family,
};
use chemifree_application::{LossDisposition, SemanticImpact};
use chemifree_providers::V2000Codec;

const COUNTS_LINE: &str = "  0  0  0     0  0            999 V3000";
const COUNTS_LINE_ZERO: &str = "  0  0  0     0  0              0 V3000";
const CORE: &str = include_str!("data/ctfile-v3000/core.mol");
const COLLECTIONS: &str = include_str!("data/ctfile-v3000/collections-crlf.mol");
const CONTINUED: &str = include_str!("data/ctfile-v3000/continued.mol");

/// A valid three-atom record; the table below changes one thing at a time. Line numbers in the
/// table follow from this layout: `BEGIN CTAB` is line 5, `COUNTS` 6, `BEGIN ATOM` 7, atoms
/// 8-10, `END ATOM` 11, `BEGIN BOND` 12, bonds 13-14, `END BOND` 15, `END CTAB` 16, `M  END` 17.
fn base() -> String {
    let body = [
        "BEGIN CTAB",
        "COUNTS 3 2 0 0 0",
        "BEGIN ATOM",
        "1 C 0 0 0 0",
        "2 C 1 0 0 0",
        "3 O 2 0 0 0",
        "END ATOM",
        "BEGIN BOND",
        "1 1 1 2",
        "2 2 2 3",
        "END BOND",
        "END CTAB",
    ];
    let mut text = format!("t\n  p\n\n{COUNTS_LINE}\n");
    for line in body {
        text.push_str("M  V30 ");
        text.push_str(line);
        text.push('\n');
    }
    text.push_str("M  END\n");
    text
}

fn read(text: &str) -> Result<Vec<CtRecord>, (&'static str, Option<usize>)> {
    V2000Codec
        .read_ctab(CtFormat::Mol, text.as_bytes())
        .map(|r| r.value)
        .map_err(|e| (e.code, e.line))
}

fn atom(element: &str, x: f64) -> CtAtom {
    CtAtom {
        element: element.into(),
        x,
        y: 0.,
        z: 0.,
        charge: 0,
        isotope: None,
        radical: CtRadical::None,
    }
}

fn record(atoms: Vec<CtAtom>, bonds: Vec<CtBond>) -> CtRecord {
    CtRecord {
        title: "T".into(),
        program: String::new(),
        comment: String::new(),
        atoms,
        bonds,
        fields: vec![],
        ctab: CtTab::V3000,
        chiral: false,
        collections: vec![],
    }
}

fn bond(begin: usize, end: usize, order: u8) -> CtBond {
    CtBond {
        begin,
        end,
        order,
        stereo: CtBondStereo::None,
    }
}

fn write(record: &CtRecord) -> Result<String, &'static str> {
    V2000Codec
        .write_ctab(CtFormat::Mol, CtTab::V3000, std::slice::from_ref(record))
        .map(|r| r.value)
        .map_err(|e| e.code)
}

#[test]
fn reads_the_core_subset() {
    let result = V2000Codec
        .read_ctab(CtFormat::Mol, CORE.as_bytes())
        .unwrap();
    let item = &result.value[0];
    assert_eq!(item.ctab, CtTab::V3000);
    assert!(item.chiral, "COUNTS chiral flag 1 is read");
    assert_eq!(
        item.atoms
            .iter()
            .map(|a| (&*a.element, a.charge, a.isotope, a.radical))
            .collect::<Vec<_>>(),
        [
            ("C", 0, None, CtRadical::None),
            ("C", 0, Some(13), CtRadical::None),
            ("O", -1, None, CtRadical::None),
            ("N", 0, None, CtRadical::Doublet),
            ("Cl", 1, Some(37), CtRadical::None),
        ]
    );
    assert_eq!(item.atoms[4].z, 0.5);
    assert_eq!(
        item.bonds,
        [bond(0, 1, 1), bond(1, 2, 2), bond(2, 3, 4), bond(3, 4, 3)]
    );
    let features: Vec<_> = result
        .losses
        .iter()
        .map(|l| (&*l.feature, &*l.source_path, l.disposition))
        .collect();
    assert_eq!(
        features,
        [
            (
                "isotope_validation",
                "records[1].atoms[2, 5].isotope",
                LossDisposition::Unknown
            ),
            (
                "coordinate_stereo",
                "records[1].bonds[2]",
                LossDisposition::Unknown
            ),
        ]
    );
}

#[test]
fn the_v2000_entry_point_still_refuses_v3000() {
    let error = V2000Codec.read(CtFormat::Mol, CORE.as_bytes()).unwrap_err();
    assert_eq!((error.code, error.line), ("unsupported_version", Some(4)));
}

#[test]
fn reads_collections_with_any_number_and_crlf_line_ends() {
    assert!(COLLECTIONS.contains("\r\n"), "fixture must use CRLF");
    let item = &read(COLLECTIONS).unwrap()[0];
    assert!(item.chiral);
    let abs = |kind, number, atoms: &[usize]| CtCollection {
        kind,
        number,
        atoms: atoms.to_vec(),
    };
    assert_eq!(
        item.collections,
        [
            abs(CtCollectionKind::Absolute, None, &[1, 3]),
            abs(CtCollectionKind::Or, Some(0), &[0]),
            abs(CtCollectionKind::And, Some(-128), &[2, 4]),
            abs(CtCollectionKind::And, Some(258), &[5]),
        ]
    );
}

#[test]
fn joins_continued_lines_without_inserting_text() {
    let item = &read(CONTINUED).unwrap()[0];
    assert_eq!((item.atoms[0].x, item.atoms[0].z), (0.0, 0.0));
    assert_eq!(
        item.atoms[1].charge, 1,
        "CHG=1 arrives on a continuation line"
    );
    assert_eq!(item.collections[0].atoms, [0, 1, 2]);
}

#[test]
fn an_sdf_may_mix_a_v2000_and_a_v3000_record() {
    let v2000 = "A\n\n\n  1  0  0  0  0  0  0  0  0  0999 V2000\n    0.0000    0.0000    0.0000 C   0  0  0  0  0  0  0  0  0  0  0  0\nM  END\n> <N>\nx\n\n$$$$\n";
    let text = format!("{v2000}{}> <N>\ny\n\n$$$$\n", base());
    let result = V2000Codec
        .read_ctab(CtFormat::Sdf, text.as_bytes())
        .unwrap();
    let tabs: Vec<_> = result
        .value
        .iter()
        .map(|r| (r.ctab, r.fields[0].value.clone()))
        .collect();
    assert_eq!(
        tabs,
        [
            (CtTab::V2000, "x".to_owned()),
            (CtTab::V3000, "y".to_owned())
        ]
    );
}

#[test]
fn a_registry_number_is_dropped_and_reported() {
    let text = base().replace("COUNTS 3 2 0 0 0", "COUNTS 3 2 0 0 0 REGNO=4711");
    let result = V2000Codec
        .read_ctab(CtFormat::Mol, text.as_bytes())
        .unwrap();
    assert!(result.losses.iter().any(|l| l.feature == "registry_number"
        && l.disposition == LossDisposition::Dropped
        && l.semantic_impact == SemanticImpact::Document));
}

/// Each row changes the valid record once. `(name, from, to, code, line)`.
#[test]
#[rustfmt::skip]
fn refuses_each_unsupported_or_malformed_construct_with_its_reason_and_line() {
    let long_number = "1".repeat(25);
    let padded = format!("M  V30 1 C 0 0 0 0{}", " ".repeat(80 - 18));
    let overlong = format!("{padded} ");
    let cases: Vec<(&str, String, String, &str, usize)> = vec![
        // Counts and blocks.
        ("atom count too small", "COUNTS 3 2", "COUNTS 4 2", "v3000_count_mismatch", 6),
        ("bond count too large", "COUNTS 3 2", "COUNTS 3 3", "v3000_count_mismatch", 6),
        ("extra atom line", "M  V30 END ATOM", "M  V30 4 C 3 0 0 0\nM  V30 END ATOM", "v3000_count_mismatch", 11),
        ("block with zero count", "COUNTS 3 2", "COUNTS 3 0", "v3000_count_mismatch", 13),
        ("four counts", "COUNTS 3 2 0 0 0", "COUNTS 3 2 0 0", "v3000_invalid_counts_line", 6),
        ("bare registry number", "COUNTS 3 2 0 0 0", "COUNTS 3 2 0 0 0 4711", "v3000_invalid_counts_line", 6),
        ("registry number not a number", "COUNTS 3 2 0 0 0", "COUNTS 3 2 0 0 0 REGNO=x", "v3000_invalid_counts_line", 6),
        ("empty registry number", "COUNTS 3 2 0 0 0", "COUNTS 3 2 0 0 0 REGNO=", "v3000_invalid_counts_line", 6),
        ("odd appendix count", COUNTS_LINE, "  0  0  0     0  0            777 V3000", "invalid_counts", 4),
        ("negative count", "COUNTS 3 2", "COUNTS -1 2", "v3000_invalid_counts_line", 6),
        ("chiral flag 2", "COUNTS 3 2 0 0 0", "COUNTS 3 2 0 0 2", "v3000_invalid_counts_line", 6),
        ("count over int32", "COUNTS 3 2", "COUNTS 3000000000 2", "v3000_invalid_counts_line", 6),
        ("atoms over cap", "COUNTS 3 2", "COUNTS 2001 2", "resource_limit", 6),
        ("bonds over cap", "COUNTS 3 2", "COUNTS 3 4001", "resource_limit", 6),
        ("sgroup count", "COUNTS 3 2 0 0 0", "COUNTS 3 2 1 0 0", "v3000_sgroup_not_supported", 6),
        ("3d count", "COUNTS 3 2 0 0 0", "COUNTS 3 2 0 1 0", "v3000_unsupported_3d", 6),
        ("sgroup block", "M  V30 END CTAB", "M  V30 BEGIN SGROUP\nM  V30 END SGROUP\nM  V30 END CTAB", "v3000_sgroup_not_supported", 16),
        ("rgroup block", "M  V30 END CTAB", "M  V30 BEGIN RGROUP\nM  V30 END CTAB", "v3000_unsupported_rgroup", 16),
        ("3d block", "M  V30 END CTAB", "M  V30 BEGIN OBJ3D\nM  V30 END CTAB", "v3000_unsupported_3d", 16),
        ("reaction block", "M  V30 END CTAB", "M  V30 BEGIN REACTANT\nM  V30 END CTAB", "v3000_unsupported_reaction", 16),
        ("unknown block", "M  V30 END CTAB", "M  V30 BEGIN TEMPLATE\nM  V30 END CTAB", "v3000_unsupported_block", 16),
        ("duplicate atom block", "M  V30 BEGIN BOND", "M  V30 BEGIN ATOM\nM  V30 END ATOM\nM  V30 BEGIN BOND", "v3000_duplicate_block", 12),
        ("wrong end", "M  V30 END BOND", "M  V30 END ATOM", "v3000_block_mismatch", 15),
        ("block open at end of ctab", "M  V30 END BOND\n", "", "v3000_block_not_closed", 15),
        ("stray line", "M  V30 END CTAB", "M  V30 HELLO\nM  V30 END CTAB", "v3000_block_mismatch", 16),
        ("no begin ctab", "M  V30 BEGIN CTAB\n", "", "v3000_missing_block", 5),
        ("no counts", "M  V30 COUNTS 3 2 0 0 0\n", "", "v3000_invalid_counts_line", 6),
        // Framing.
        ("missing prefix", "M  V30 2 C 1", "M  V3O 2 C 1", "v3000_missing_prefix", 9),
        ("v2000 property inside", "M  V30 END CTAB", "M  CHG  1   1   1\nM  V30 END CTAB", "v3000_missing_prefix", 16),
        ("hyphen at end of file", "M  V30 END CTAB\nM  END\n", "M  V30 END CTAB -", "v3000_unterminated_continuation", 16),
        ("hyphen before an unprefixed line", "M  V30 END CTAB", "M  V30 END CTAB -", "v3000_unterminated_continuation", 17),
        ("non-ascii", "3 O 2", "3 \u{d6} 2", "invalid_columns", 10),
        ("line of 81 columns", "M  V30 1 C 0 0 0 0", &overlong, "invalid_columns", 8),
        ("trailing data after M END", "M  END\n", "M  END junk\n", "invalid_m_end", 17),
        ("second ctab", "M  END\n", "M  V30 BEGIN CTAB\nM  END\n", "v3000_multiple_structures", 17),
        ("no M END", "M  END\n", "$$$$\n", "missing_m_end", 17),
        // Counts line.
        ("version 3001", "V3000", "V3001", "unsupported_version", 4),
        ("short counts line", COUNTS_LINE, "  0  0  0", "truncated_counts", 4),
        ("counts line garbage", COUNTS_LINE, "  x  0  0     0  0              0 V3000", "invalid_counts", 4),
        // Atoms.
        ("duplicate atom index", "2 C 1", "1 C 1", "v3000_duplicate_index", 9),
        ("zero atom index", "2 C 1", "0 C 1", "v3000_invalid_index", 9),
        ("negative atom index", "2 C 1", "-2 C 1", "v3000_invalid_index", 9),
        ("huge atom index", "2 C 1", "99999999999 C 1", "v3000_invalid_number", 9),
        ("25 character coordinate", "2 C 1 0 0 0", &format!("2 C {long_number} 0 0 0"), "v3000_invalid_number", 9),
        ("nan coordinate", "2 C 1 0 0 0", "2 C nan 0 0 0", "v3000_invalid_number", 9),
        ("short atom line", "2 C 1 0 0 0", "2 C 1 0 0", "v3000_invalid_atom", 9),
        ("r group atom", "3 O 2", "3 R# 2", "v3000_unsupported_rgroup", 10),
        ("any atom", "3 O 2", "3 A 2", "v3000_unsupported_query", 10),
        ("atom list", "3 O 2", "3 [C,N] 2", "v3000_unsupported_query", 10),
        ("negated list", "3 O 2", "3 NOT [C,N] 2", "v3000_unsupported_query", 10),
        ("unknown element", "3 O 2", "3 Xx 2", "v3000_unsupported_atom", 10),
        ("atom map", "3 O 2 0 0 0", "3 O 2 0 0 5", "v3000_unsupported_atom_property", 10),
        ("valence", "3 O 2 0 0 0", "3 O 2 0 0 0 VAL=2", "v3000_unsupported_atom_property", 10),
        ("atom parity", "3 O 2 0 0 0", "3 O 2 0 0 0 CFG=1", "v3000_unsupported_atom_property", 10),
        ("hydrogen count query", "3 O 2 0 0 0", "3 O 2 0 0 0 HCOUNT=1", "v3000_unsupported_query", 10),
        ("attachment point", "3 O 2 0 0 0", "3 O 2 0 0 0 ATTCHPT=1", "v3000_unsupported_rgroup", 10),
        ("charge over i8", "3 O 2 0 0 0", "3 O 2 0 0 0 CHG=1000", "v3000_invalid_charge", 10),
        ("charge 16", "3 O 2 0 0 0", "3 O 2 0 0 0 CHG=16", "v3000_invalid_charge", 10),
        ("charge -16", "3 O 2 0 0 0", "3 O 2 0 0 0 CHG=-16", "v3000_invalid_charge", 10),
        ("radical 4", "3 O 2 0 0 0", "3 O 2 0 0 0 RAD=4", "v3000_invalid_radical", 10),
        ("mass zero", "3 O 2 0 0 0", "3 O 2 0 0 0 MASS=0", "v3000_invalid_isotope", 10),
        ("repeated property", "3 O 2 0 0 0", "3 O 2 0 0 0 CHG=1 CHG=2", "v3000_duplicate_property", 10),
        ("property without value", "3 O 2 0 0 0", "3 O 2 0 0 0 CHG", "v3000_invalid_property", 10),
        ("unclosed parenthesis", "3 O 2 0 0 0", "3 O 2 0 0 0 ENDPTS=(2 1", "v3000_invalid_property", 10),
        ("unclosed quote", "3 O 2 0 0 0", "3 O 2 0 0 0 NAME=\"x", "v3000_invalid_property", 10),
        // Bonds.
        ("forward reference", "1 1 1 2", "1 1 1 4", "v3000_unknown_atom_index", 13),
        ("bond to a missing atom", "2 2 2 3", "2 2 2 9", "v3000_unknown_atom_index", 14),
        ("duplicate bond index", "2 2 2 3", "1 2 2 3", "v3000_duplicate_index", 14),
        ("query bond type", "2 2 2 3", "2 5 2 3", "v3000_unsupported_query", 14),
        ("dative bond type", "2 2 2 3", "2 9 2 3", "v3000_unsupported_bond", 14),
        ("bond type zero", "2 2 2 3", "2 0 2 3", "v3000_unsupported_bond", 14),
        ("wedge on a double bond", "2 2 2 3", "2 2 2 3 CFG=1", "v3000_unsupported_bond_property", 14),
        ("hash on an aromatic bond", "2 2 2 3", "2 4 2 3 CFG=3", "v3000_unsupported_bond_property", 14),
        ("bond CFG out of range", "2 2 2 3", "2 1 2 3 CFG=4", "v3000_invalid_bond", 14),
        ("ring topology", "2 2 2 3", "2 1 2 3 TOPO=1", "v3000_unsupported_query", 14),
        ("self bond", "2 2 2 3", "2 2 2 2", "v3000_invalid_bond", 14),
        ("repeated pair", "2 2 2 3", "2 2 2 1", "v3000_duplicate_bond", 14),
        ("short bond line", "2 2 2 3", "2 2 2", "v3000_invalid_bond", 14),
    ]
    .into_iter()
    .map(|(name, from, to, code, line)| (name, from.to_owned(), to.to_owned(), code, line))
    .collect();
    for (name, from, to, code, line) in cases {
        let text = base();
        assert!(text.contains(&from), "{name}: fixture lacks {from:?}");
        let text = text.replacen(&from, &to, 1);
        assert_eq!(
            read(&text).map(|_| ()),
            Err((code, Some(line))),
            "{name}: expected {code} at line {line}"
        );
    }
}

#[test]
fn a_bond_before_the_atom_block_is_a_dangling_reference() {
    let text = base()
        .replace("M  V30 BEGIN BOND\nM  V30 1 1 1 2\nM  V30 2 2 2 3\nM  V30 END BOND\n", "")
        .replace(
            "M  V30 BEGIN ATOM\n",
            "M  V30 BEGIN BOND\nM  V30 1 1 1 2\nM  V30 2 2 2 3\nM  V30 END BOND\nM  V30 BEGIN ATOM\n",
        );
    assert_eq!(
        read(&text).map(|_| ()),
        Err(("v3000_unknown_atom_index", Some(8)))
    );
}

#[test]
#[rustfmt::skip]
fn refuses_each_unsupported_collection_with_its_reason() {
    let with = |line: &str| {
        base().replacen(
            "M  V30 END CTAB",
            &format!(
                "M  V30 BEGIN COLLECTION\nM  V30 {line}\nM  V30 END COLLECTION\nM  V30 END CTAB"
            ),
            1,
        )
    };
    let cases = [
        ("blank collection line", "", "v3000_invalid_collection"),
        ("highlight collection", "MDLV30/HILITE ATOMS=(1 1)", "v3000_collection_not_supported_yet"),
        ("bond members", "MDLV30/STEREL1 ATOMS=(1 1) BONDS=(1 1)", "v3000_collection_not_supported_yet"),
        ("no atoms key", "MDLV30/STEABS", "v3000_invalid_collection"),
        ("repeated atoms key", "MDLV30/STEABS ATOMS=(1 1) ATOMS=(1 2)", "v3000_invalid_collection"),
        ("number over int32", "MDLV30/STEREL99999999999 ATOMS=(1 1)", "v3000_invalid_collection"),
        ("missing number", "MDLV30/STERAC ATOMS=(1 1)", "v3000_invalid_collection"),
        ("count mismatch", "MDLV30/STEABS ATOMS=(2 1)", "v3000_count_mismatch"),
        ("unknown atom", "MDLV30/STEABS ATOMS=(1 9)", "v3000_unknown_atom_index"),
        ("atom twice in one", "MDLV30/STEABS ATOMS=(2 1 1)", "v3000_duplicate_index"),
        ("empty collection", "MDLV30/STEABS ATOMS=(0)", "v3000_empty_collection"),
    ];
    for (name, line, code) in cases {
        assert_eq!(
            read(&with(line)).map(|_| ()),
            Err((code, Some(17))),
            "{name}: expected {code}"
        );
    }
}

#[test]
fn every_refusal_reason_has_a_closed_diagnostic_family() {
    let families = [
        "malformed_input",
        "unsupported_feature",
        "unsupported_format",
        "dangling_reference",
        "resource_limit",
    ];
    for (code, family) in CT_REASON_FAMILIES {
        assert!(
            families.contains(family),
            "{code} maps to unknown family {family}"
        );
    }
    let mut codes: Vec<_> = CT_REASON_FAMILIES.iter().map(|(c, _)| *c).collect();
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(
        codes.len(),
        CT_REASON_FAMILIES.len(),
        "a reason is listed twice"
    );
    // The reasons the table above and the writer tests rely on.
    for code in [
        "v3000_missing_prefix",
        "v3000_unknown_atom_index",
        "v3000_sgroup_not_supported",
        "v3000_collection_not_supported_yet",
        "resource_limit",
        "unsupported_version",
    ] {
        assert!(ct_reason_family(code).is_some(), "{code} is not mapped");
    }
    assert_eq!(
        ct_reason_family("v3000_unknown_atom_index"),
        Some("dangling_reference")
    );
    assert_eq!(
        ct_reason_family("v3000_sgroup_not_supported"),
        Some("unsupported_feature")
    );
    assert_eq!(
        ct_reason_family("v3000_count_mismatch"),
        Some("malformed_input")
    );
    assert_eq!(ct_reason_family("not_a_reason"), None);
}

#[test]
fn cutting_a_valid_file_at_any_byte_never_panics_or_yields_a_partial_record() {
    let text = base();
    let full = read(&text).unwrap();
    for end in 0..text.len() {
        match read(&text[..end]) {
            Err(_) => {}
            Ok(records) => assert_eq!(records, full, "cut at byte {end} gave a different record"),
        }
    }
}

#[test]
fn accepts_the_size_limits_exactly_and_refuses_one_more() {
    let atoms = |n: usize| (0..n).map(|i| atom("C", i as f64)).collect::<Vec<_>>();
    let chain = |n: usize, extra: usize| {
        // n - 1 chain bonds, then `extra` bonds between atoms two apart.
        let mut bonds: Vec<_> = (0..n - 1).map(|i| bond(i, i + 1, 1)).collect();
        bonds.extend(
            (2..4)
                .flat_map(|gap| (0..n - gap).map(move |i| (i, i + gap)))
                .take(extra)
                .map(|(i, j)| bond(i, j, 1)),
        );
        bonds
    };
    let at_limit = record(atoms(2000), chain(2000, 4000 - 1999));
    let text = write(&at_limit).unwrap();
    assert_eq!(read(&text).unwrap()[0].bonds.len(), 4000);
    assert_eq!(write(&record(atoms(2001), vec![])), Err("resource_limit"));
    assert_eq!(
        write(&record(atoms(2000), chain(2000, 4000 - 1999 + 1))),
        Err("resource_limit")
    );
}

#[test]
fn an_atom_line_of_exactly_80_columns_is_accepted() {
    let line = format!("M  V30 1 C 0 0 0 0{}", " ".repeat(80 - 18));
    assert_eq!(line.len(), 80);
    let text = base().replacen("M  V30 1 C 0 0 0 0", &line, 1);
    assert_eq!(read(&text).unwrap()[0].atoms.len(), 3);
}

#[test]
fn writes_the_documented_layout_byte_for_byte() {
    let mut oxygen = atom("O", 1.5);
    oxygen.charge = -1;
    oxygen.isotope = Some(18);
    oxygen.radical = CtRadical::Doublet;
    let item = record(
        vec![atom("C", 0.0), oxygen, atom("N", 3.0)],
        vec![bond(0, 1, 1), bond(1, 2, 4)],
    );
    let text = write(&item).unwrap();
    let expected = format!(
        "T\n\n\n{COUNTS_LINE}\n\
M  V30 BEGIN CTAB\n\
M  V30 COUNTS 3 2 0 0 0\n\
M  V30 BEGIN ATOM\n\
M  V30 1 C 0.0000 0.0000 0.0000 0\n\
M  V30 2 O 1.5000 0.0000 0.0000 0 CHG=-1 RAD=2 MASS=18\n\
M  V30 3 N 3.0000 0.0000 0.0000 0\n\
M  V30 END ATOM\n\
M  V30 BEGIN BOND\n\
M  V30 1 1 1 2\n\
M  V30 2 4 2 3\n\
M  V30 END BOND\n\
M  V30 END CTAB\n\
M  END\n"
    );
    assert_eq!(text, expected);
    let counts = text.lines().nth(3).unwrap();
    assert_eq!((counts.len(), &counts[34..39]), (39, "V3000"));
    assert_eq!(write(&item).unwrap(), text, "same input, same bytes");
}

#[test]
fn an_empty_record_omits_its_empty_blocks() {
    let text = write(&record(vec![], vec![])).unwrap();
    assert_eq!(
        text,
        format!(
            "T\n\n\n{COUNTS_LINE}\nM  V30 BEGIN CTAB\nM  V30 COUNTS 0 0 0 0 0\nM  V30 END CTAB\nM  END\n"
        )
    );
    assert_eq!(read(&text).unwrap()[0].atoms.len(), 0);
}

#[test]
fn round_trips_through_the_reader_and_through_crlf() {
    let mut oxygen = atom("O", 1.5);
    oxygen.charge = -1;
    oxygen.isotope = Some(18);
    oxygen.radical = CtRadical::Triplet;
    let mut carbon = atom("C", -2.25);
    carbon.y = 3.125;
    carbon.z = -0.5;
    let item = record(
        vec![carbon, oxygen, atom("N", 3.0)],
        vec![bond(2, 0, 3), bond(0, 1, 4)],
    );
    let text = write(&item).unwrap();
    assert_eq!(read(&text).unwrap(), std::slice::from_ref(&item));
    assert_eq!(read(&text.replace('\n', "\r\n")).unwrap(), [item]);
}

#[test]
fn continues_a_long_logical_line_and_reads_it_back() {
    let mut far = atom("C", 1.0e15);
    far.y = -1.0e15;
    far.z = 1.0e15;
    far.charge = -10;
    far.isotope = Some(999);
    far.radical = CtRadical::Doublet;
    let item = record(vec![far], vec![]);
    let text = write(&item).unwrap();
    let physical: Vec<_> = text.lines().filter(|l| l.starts_with("M  V30 ")).collect();
    assert!(
        physical.iter().all(|l| l.len() <= 80),
        "a physical line passes 80 columns: {physical:?}"
    );
    let continued: Vec<_> = physical.iter().filter(|l| l.ends_with(" -")).collect();
    assert_eq!(
        continued.len(),
        1,
        "the atom line continues once: {physical:?}"
    );
    assert_eq!(read(&text).unwrap(), [item]);
}

#[test]
fn writer_refusals_and_reports() {
    let two = || record(vec![atom("C", 0.0), atom("C", 1.0)], vec![bond(0, 1, 1)]);
    let mut aromatic_five = two();
    aromatic_five.bonds[0].order = 5;
    assert_eq!(write(&aromatic_five), Err("unsupported_bond"));
    let mut unknown = two();
    unknown.atoms[0].element = "Xx".into();
    assert_eq!(write(&unknown), Err("unsupported_atom"));
    for charge in [16, -16, 127, -128] {
        let mut charged = two();
        charged.atoms[0].charge = charge;
        assert_eq!(write(&charged), Err("invalid_charge"), "charge {charge}");
    }
    for charge in [15, -15] {
        let mut charged = two();
        charged.atoms[0].charge = charge;
        assert!(write(&charged).is_ok(), "charge {charge}");
    }
    let mut nan = two();
    nan.atoms[0].x = f64::NAN;
    assert_eq!(write(&nan), Err("invalid_coordinate"));
    let mut huge = two();
    huge.atoms[0].x = 1.0e300;
    assert_eq!(write(&huge), Err("invalid_coordinate"));
    let mut loop_bond = two();
    loop_bond.bonds[0].end = 0;
    assert_eq!(write(&loop_bond), Err("invalid_bond"));
    let mut twice = two();
    twice.bonds.push(bond(1, 0, 2));
    assert_eq!(write(&twice), Err("duplicate_bond"));
    let mut grouped = two();
    grouped.collections.push(CtCollection {
        kind: CtCollectionKind::Absolute,
        number: None,
        atoms: vec![0],
    });
    assert!(write(&grouped).is_ok(), "V3000 writes collections now");
    assert_eq!(
        V2000Codec
            .write_ctab(CtFormat::Mol, CtTab::V2000, std::slice::from_ref(&grouped))
            .map_err(|e| e.code),
        Err("v3000_collection_not_supported_yet"),
        "V2000 does not drop collections either"
    );
    let mut chiral = two();
    chiral.chiral = true;
    let v2000 = V2000Codec
        .write_ctab(CtFormat::Mol, CtTab::V2000, std::slice::from_ref(&chiral))
        .unwrap();
    assert!(
        v2000.losses.iter().any(|l| l.feature == "chiral_flag"
            && l.disposition == LossDisposition::Dropped
            && l.semantic_impact == SemanticImpact::Stereo),
        "the V2000 writer states 0 and reports the flag"
    );
    let v3000 = V2000Codec
        .write_ctab(CtFormat::Mol, CtTab::V3000, std::slice::from_ref(&chiral))
        .unwrap();
    assert!(
        v3000.losses.iter().all(|l| l.feature != "chiral_flag"),
        "the V3000 writer states the flag, so nothing is dropped"
    );
    assert!(
        v3000.value.contains("M  V30 COUNTS 2 1 0 0 1\n"),
        "{}",
        v3000.value
    );
    let mut rounded = two();
    rounded.atoms[0].x = 0.123456;
    let result = V2000Codec
        .write_ctab(CtFormat::Mol, CtTab::V3000, std::slice::from_ref(&rounded))
        .unwrap();
    assert!(
        result
            .losses
            .iter()
            .any(|l| l.feature == "coordinate_precision")
    );
}

#[test]
fn the_v2000_writer_is_unchanged_by_the_selector() {
    let item = record(vec![atom("C", 0.0), atom("C", 1.0)], vec![bond(0, 1, 1)]);
    let direct = V2000Codec
        .write(CtFormat::Mol, std::slice::from_ref(&item))
        .unwrap();
    let selected = V2000Codec
        .write_ctab(CtFormat::Mol, CtTab::V2000, std::slice::from_ref(&item))
        .unwrap();
    assert_eq!(direct.value, selected.value);
    assert!(direct.value.contains(" V2000\n"));
    assert_eq!(write(&item).unwrap().matches("V3000").count(), 1);
}

#[test]
fn text_after_m_end_of_a_mol_file_is_refused() {
    let text = format!("{}junk\n", base());
    assert_eq!(read(&text).map(|_| ()), Err(("trailing_content", Some(18))));
}

#[test]
fn reads_both_spellings_of_the_appendix_count() {
    let zero = base().replace(COUNTS_LINE, COUNTS_LINE_ZERO);
    assert_ne!(zero, base());
    assert_eq!(read(&zero).unwrap(), read(&base()).unwrap());
    assert_eq!(
        write(&record(vec![], vec![])).unwrap().lines().nth(3),
        Some(COUNTS_LINE)
    );
}

#[test]
fn a_v3000_double_bond_loss_does_not_speak_of_v2000() {
    let text = base();
    let read_loss = V2000Codec
        .read_ctab(CtFormat::Mol, text.as_bytes())
        .unwrap()
        .losses;
    let item = record(vec![atom("C", 0.0), atom("C", 1.0)], vec![bond(0, 1, 2)]);
    let written = V2000Codec
        .write_ctab(CtFormat::Mol, CtTab::V3000, std::slice::from_ref(&item))
        .unwrap()
        .losses;
    assert_eq!(
        written
            .iter()
            .filter(|l| l.feature == "coordinate_stereo")
            .count(),
        1
    );
    for loss in read_loss
        .iter()
        .chain(&written)
        .filter(|l| l.feature == "coordinate_stereo")
    {
        assert!(
            !loss.reason.contains("V2000"),
            "V3000 loss mentions V2000: {}",
            loss.reason
        );
    }
    let v2000 = V2000Codec
        .write_ctab(CtFormat::Mol, CtTab::V2000, std::slice::from_ref(&item))
        .unwrap()
        .losses;
    assert_eq!(
        v2000[0].reason,
        "V2000 double-bond stereo code 0 can be interpreted from coordinates; this codec does not infer or preserve an E/Z assignment."
    );
}

#[test]
fn many_isotopes_give_one_aggregated_loss_on_read_and_on_write() {
    let mut atoms = vec![atom("C", 0.0), atom("C", 1.0), atom("C", 2.0)];
    for a in &mut atoms {
        a.isotope = Some(13);
    }
    let item = record(atoms, vec![]);
    let written = V2000Codec
        .write_ctab(CtFormat::Mol, CtTab::V3000, std::slice::from_ref(&item))
        .unwrap();
    let paths: Vec<_> = written
        .losses
        .iter()
        .map(|l| l.source_path.as_str())
        .collect();
    assert_eq!(paths, ["records[1].atoms[1, 2, 3].isotope"]);
    let read_back = V2000Codec
        .read_ctab(CtFormat::Mol, written.value.as_bytes())
        .unwrap();
    let paths: Vec<_> = read_back
        .losses
        .iter()
        .map(|l| l.source_path.as_str())
        .collect();
    assert_eq!(paths, ["records[1].atoms[1, 2, 3].isotope"]);
}

#[test]
fn every_v3000_reason_in_the_source_has_a_family_and_the_table_lists_no_other() {
    let source = include_str!("../src/ctfile/v3000.rs");
    let mut used = std::collections::BTreeSet::new();
    for part in source.split('"') {
        if part.starts_with("v3000_")
            && part
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b == b'_' || b.is_ascii_digit())
        {
            assert!(
                ct_reason_family(part).is_some(),
                "{part} has no diagnostic family"
            );
            used.insert(part);
        }
    }
    // Reasons the application's projection owns (it reads the codec's record, not the file).
    const PROJECTION_OWNED: [&str; 1] = ["v3000_collection_member_not_stereocenter"];
    let listed: std::collections::BTreeSet<_> = CT_REASON_FAMILIES
        .iter()
        .map(|(code, _)| *code)
        .filter(|code| code.starts_with("v3000_") && !PROJECTION_OWNED.contains(code))
        .collect();
    assert_eq!(used, listed, "the table and the V3000 source disagree");
}

#[test]
fn reads_the_charge_boundaries_and_refuses_beyond_them() {
    for charge in [15, -15] {
        let text = CORE.replacen(" CHG=-1", &format!(" CHG={charge}"), 1);
        assert!(
            text.contains(&format!("CHG={charge}")),
            "core.mol has a CHG=-1 atom to replace"
        );
        let records = read(&text).unwrap_or_else(|e| panic!("CHG={charge}: {e:?}"));
        assert!(
            records
                .iter()
                .any(|r| r.atoms.iter().any(|a| i32::from(a.charge) == charge))
        );
    }
}

fn marked(order: u8, stereo: CtBondStereo) -> CtRecord {
    let mut bond = bond(0, 1, order);
    bond.stereo = stereo;
    record(vec![atom("C", 0.0), atom("C", 1.0)], vec![bond])
}

#[test]
fn reads_bond_cfg_as_wedge_either_and_hash() {
    let text = base()
        .replace("1 1 1 2\n", "1 1 1 2 CFG=1\n")
        .replace("2 2 2 3\n", "2 2 2 3 CFG=2\n");
    let item = &read(&text).unwrap()[0];
    assert_eq!(item.bonds[0].stereo, CtBondStereo::Wedge);
    assert_eq!(item.bonds[1].stereo, CtBondStereo::Either);
    let hash = base().replace("1 1 1 2\n", "1 1 1 2 CFG=3\n");
    assert_eq!(read(&hash).unwrap()[0].bonds[0].stereo, CtBondStereo::Hash);
    let zero = base().replace("1 1 1 2\n", "1 1 1 2 CFG=0\n");
    assert_eq!(read(&zero).unwrap()[0].bonds[0].stereo, CtBondStereo::None);
}

#[test]
fn v3000_writes_cfg_one_two_and_three() {
    for (order, stereo, line) in [
        (1, CtBondStereo::Wedge, "M  V30 1 1 1 2 CFG=1\n"),
        (1, CtBondStereo::Hash, "M  V30 1 1 1 2 CFG=3\n"),
        (1, CtBondStereo::Either, "M  V30 1 1 1 2 CFG=2\n"),
        (2, CtBondStereo::Either, "M  V30 1 2 1 2 CFG=2\n"),
        (2, CtBondStereo::None, "M  V30 1 2 1 2\n"),
    ] {
        let text = write(&marked(order, stereo)).unwrap();
        assert!(text.contains(line), "{stereo:?} on {order}: {text}");
    }
}

#[test]
fn v2000_writes_the_bond_stereo_column_one_six_four_and_three() {
    for (order, stereo, line) in [
        (1, CtBondStereo::Wedge, "  1  2  1  1  0  0  0\n"),
        (1, CtBondStereo::Hash, "  1  2  1  6  0  0  0\n"),
        (1, CtBondStereo::Either, "  1  2  1  4  0  0  0\n"),
        (2, CtBondStereo::Either, "  1  2  2  3  0  0  0\n"),
        (2, CtBondStereo::None, "  1  2  2  0  0  0  0\n"),
    ] {
        let item = marked(order, stereo);
        let text = V2000Codec
            .write_ctab(CtFormat::Mol, CtTab::V2000, std::slice::from_ref(&item))
            .unwrap()
            .value;
        assert!(text.contains(line), "{stereo:?} on {order}: {text}");
    }
}

#[test]
fn a_mark_a_bond_cannot_carry_is_refused_on_write_in_both_dialects() {
    for (order, stereo) in [
        (2, CtBondStereo::Wedge),
        (2, CtBondStereo::Hash),
        (3, CtBondStereo::Either),
        (4, CtBondStereo::Wedge),
    ] {
        let item = marked(order, stereo);
        assert_eq!(
            write(&item),
            Err("unsupported_bond"),
            "{stereo:?} on {order}"
        );
        let v2000 = V2000Codec.write_ctab(CtFormat::Mol, CtTab::V2000, std::slice::from_ref(&item));
        if order != 4 {
            assert_eq!(v2000.unwrap_err().code, "unsupported_bond");
        }
    }
}

#[test]
fn an_either_double_bond_reports_no_coordinate_stereo_and_a_plain_one_does() {
    let has = |item: &CtRecord| {
        V2000Codec
            .write_ctab(CtFormat::Mol, CtTab::V3000, std::slice::from_ref(item))
            .unwrap()
            .losses
            .iter()
            .any(|l| l.feature == "coordinate_stereo")
    };
    assert!(!has(&marked(2, CtBondStereo::Either)));
    assert!(has(&marked(2, CtBondStereo::None)));
}

/// A file with three atoms and the given collection lines.
fn grouped_file(lines: &[&str]) -> String {
    let body: String = lines.iter().map(|l| format!("M  V30 {l}\n")).collect();
    base().replacen(
        "M  V30 END CTAB",
        &format!("M  V30 BEGIN COLLECTION\n{body}M  V30 END COLLECTION\nM  V30 END CTAB"),
        1,
    )
}

#[test]
#[rustfmt::skip]
fn refuses_collections_that_repeat_an_identity_or_an_atom() {
    let cases: [(&str, &[&str], &str); 5] = [
        ("two STEABS", &["MDLV30/STEABS ATOMS=(1 1)", "MDLV30/STEABS ATOMS=(1 2)"], "v3000_duplicate_collection"),
        ("same type and index", &["MDLV30/STERAC1 ATOMS=(1 1)", "MDLV30/STERAC1 ATOMS=(1 2)"], "v3000_duplicate_collection"),
        ("one atom in and1 and or1", &["MDLV30/STERAC1 ATOMS=(1 1)", "MDLV30/STEREL1 ATOMS=(1 1)"], "v3000_collection_atom_shared"),
        ("atom in absolute and and", &["MDLV30/STEABS ATOMS=(1 2)", "MDLV30/STERAC1 ATOMS=(2 1 2)"], "v3000_collection_atom_shared"),
        ("stated zero twice", &["MDLV30/STEREL0 ATOMS=(1 1)", "MDLV30/STEREL0 ATOMS=(1 2)"], "v3000_duplicate_collection"),
    ];
    for (name, lines, code) in cases {
        let text = grouped_file(lines);
        let found = read(&text).map(|_| ()).map_err(|(c, _)| c);
        assert_eq!(found, Err(code), "{name}");
    }
    // The same index of different kinds, and different indices, are separate collections.
    let fine = grouped_file(&["MDLV30/STERAC1 ATOMS=(1 1)", "MDLV30/STEREL1 ATOMS=(1 2)", "MDLV30/STERAC2 ATOMS=(1 3)"]);
    assert_eq!(read(&fine).unwrap()[0].collections.len(), 3);
}

#[test]
fn writes_collections_in_the_records_order_with_sorted_members_and_the_chiral_flag() {
    let mut item = record(
        vec![
            atom("C", 0.0),
            atom("C", 1.0),
            atom("C", 2.0),
            atom("C", 3.0),
        ],
        vec![bond(0, 1, 1)],
    );
    item.chiral = true;
    item.collections = vec![
        CtCollection {
            kind: CtCollectionKind::Absolute,
            number: None,
            atoms: vec![3, 0],
        },
        CtCollection {
            kind: CtCollectionKind::And,
            number: Some(-7),
            atoms: vec![2],
        },
        CtCollection {
            kind: CtCollectionKind::Or,
            number: Some(0),
            atoms: vec![1],
        },
    ];
    let text = write(&item).unwrap();
    assert!(text.contains("M  V30 COUNTS 4 1 0 0 1\n"), "{text}");
    assert!(
        text.contains(
            "M  V30 END BOND\nM  V30 BEGIN COLLECTION\nM  V30 MDLV30/STEABS ATOMS=(2 1 4)\nM  V30 MDLV30/STERAC-7 ATOMS=(1 3)\nM  V30 MDLV30/STEREL0 ATOMS=(1 2)\nM  V30 END COLLECTION\nM  V30 END CTAB\n"
        ),
        "{text}"
    );
    let back = &read(&text).unwrap()[0];
    assert!(back.chiral);
    assert_eq!(back.collections.len(), 3);
    assert_eq!(back.collections[0].atoms, [0, 3]);
    assert_eq!(
        (back.collections[1].kind, back.collections[1].number),
        (CtCollectionKind::And, Some(-7))
    );
}

#[test]
fn the_writer_refuses_collections_the_reader_would_refuse() {
    let base_record = || {
        record(
            vec![atom("C", 0.0), atom("C", 1.0), atom("C", 2.0)],
            vec![bond(0, 1, 1)],
        )
    };
    let collection = |kind, number, atoms: &[usize]| CtCollection {
        kind,
        number,
        atoms: atoms.to_vec(),
    };
    let cases = [
        (
            "empty",
            vec![collection(CtCollectionKind::Absolute, None, &[])],
            "v3000_empty_collection",
        ),
        (
            "atom out of range",
            vec![collection(CtCollectionKind::Absolute, None, &[3])],
            "v3000_unknown_atom_index",
        ),
        (
            "member twice",
            vec![collection(CtCollectionKind::Absolute, None, &[1, 1])],
            "v3000_duplicate_index",
        ),
        (
            "two absolute",
            vec![
                collection(CtCollectionKind::Absolute, None, &[0]),
                collection(CtCollectionKind::Absolute, None, &[1]),
            ],
            "v3000_duplicate_collection",
        ),
        (
            "same type and number",
            vec![
                collection(CtCollectionKind::Or, Some(1), &[0]),
                collection(CtCollectionKind::Or, Some(1), &[1]),
            ],
            "v3000_duplicate_collection",
        ),
        (
            "atom in two",
            vec![
                collection(CtCollectionKind::Or, Some(1), &[0]),
                collection(CtCollectionKind::And, Some(1), &[0]),
            ],
            "v3000_collection_atom_shared",
        ),
        (
            "absolute with a number",
            vec![collection(CtCollectionKind::Absolute, Some(1), &[0])],
            "v3000_invalid_collection",
        ),
        (
            "and without a number",
            vec![collection(CtCollectionKind::And, None, &[0])],
            "v3000_invalid_collection",
        ),
    ];
    for (name, collections, code) in cases {
        let mut item = base_record();
        item.collections = collections;
        assert_eq!(write(&item), Err(code), "{name}");
    }
}
