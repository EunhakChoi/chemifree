//! Notation-level tests of the SMILES codec: no chemistry model is involved.

use std::collections::HashSet;

use chemifree_application::smiles::{
    MAX_BRANCH_DEPTH, MAX_SMILES_BYTES, MAX_SMILES_ITEMS, SmilesBondKind, SmilesCodec,
    SmilesErrorCode, SmilesFormatError, SmilesMolecule,
};
use chemifree_providers::SmilesTextCodec;

fn read(text: &str) -> SmilesMolecule {
    SmilesTextCodec
        .read(text)
        .unwrap_or_else(|e| panic!("{text:?} should read: {e:?}"))
        .molecule
}

fn error(text: &str) -> (SmilesErrorCode, usize, usize) {
    let e = SmilesTextCodec
        .read(text)
        .expect_err(&format!("{text:?} should be refused"));
    assert!(
        text.is_char_boundary(e.start) && text.is_char_boundary(e.end) && e.start <= e.end,
        "{text:?}: span {}..{} is not on char boundaries",
        e.start,
        e.end
    );
    (e.code, e.start, e.end)
}

fn kinds(m: &SmilesMolecule) -> Vec<SmilesBondKind> {
    m.bonds.iter().map(|b| b.kind).collect()
}

#[test]
fn reads_branches_rings_and_components() {
    let m = read("CC(=O)O.[Na+].[Cl-]");
    assert_eq!(m.atoms.len(), 6);
    assert_eq!(m.bonds.len(), 3);
    assert_eq!(m.bonds[1].kind, SmilesBondKind::Double);
    assert_eq!((m.bonds[1].begin, m.bonds[1].end), (1, 2));
    let ring = read("C1CC1");
    assert_eq!(ring.bonds.len(), 3);
    let wide = read("C%10CC%10");
    assert_eq!(wide.bonds.len(), 3);
    // A ring may close across a dot.
    let across = read("C1.C1");
    assert_eq!(across.bonds.len(), 1);
}

#[test]
fn reads_bracket_atoms() {
    let m = read("[13CH3-][2H][O-2][N+0][C--][Fe++]");
    let a = &m.atoms;
    assert_eq!(
        (a[0].isotope, a[0].hydrogens, a[0].charge),
        (Some(13), Some(3), -1)
    );
    assert_eq!(
        (a[1].isotope, a[1].element.symbol(), a[1].hydrogens),
        (Some(2), "H", Some(0))
    );
    assert_eq!(a[2].charge, -2);
    assert_eq!(a[3].charge, 0, "+0 is allowed by the grammar");
    assert_eq!(a[4].charge, -2);
    assert_eq!(a[5].charge, 2);
    assert_eq!(read("[nH]1cccc1").atoms[0].hydrogens, Some(1));
    assert_eq!(read("[HH]").atoms[0].hydrogens, Some(1));
    assert_eq!(read("[Cn]").atoms[0].element.symbol(), "Cn");
    assert_eq!(read("[Co]").atoms[0].element.symbol(), "Co");
}

#[test]
fn unspecified_bond_between_aromatic_atoms_is_aromatic_only_in_a_ring() {
    let m = read("c1ccccc1c1ccccc1");
    let aromatic = kinds(&m)
        .iter()
        .filter(|k| **k == SmilesBondKind::Aromatic)
        .count();
    assert_eq!(aromatic, 12, "two six-membered rings");
    assert_eq!(m.bonds.len(), 13);
    assert_eq!(
        m.bonds[5].kind,
        SmilesBondKind::Aromatic,
        "ring closure bond"
    );
    assert_eq!(
        m.bonds[6].kind,
        SmilesBondKind::Single,
        "the biaryl bond is not in a ring"
    );
    assert_eq!(kinds(&read("c1ccccc1-c1ccccc1"))[6], SmilesBondKind::Single);
}

#[test]
fn bond_symbols_may_precede_ring_digits() {
    let m = read("C=1CCCCC=1");
    assert_eq!(m.bonds[5].kind, SmilesBondKind::Double);
    assert_eq!(read("C=1CCCCC1").bonds[5].kind, SmilesBondKind::Double);
    assert_eq!(error("C=1CCCCC#1").0, SmilesErrorCode::RingBondMismatch);
}

#[test]
fn whitespace_ends_the_smiles_and_later_text_is_reported() {
    let read = SmilesTextCodec.read("CCO  ethanol, a name\n").unwrap();
    assert_eq!(read.molecule.atoms.len(), 3);
    let (start, end) = read.ignored.expect("text after the terminator is reported");
    assert_eq!(&"CCO  ethanol, a name\n"[start..end], "ethanol, a name");
    assert_eq!(SmilesTextCodec.read("CCO \t\n").unwrap().ignored, None);
    assert_eq!(SmilesTextCodec.read("CCO").unwrap().ignored, None);
}

#[test]
fn empty_and_leading_whitespace_have_their_own_errors_and_spans() {
    assert_eq!(error(""), (SmilesErrorCode::EmptyInput, 0, 0));
    assert_eq!(error("   "), (SmilesErrorCode::LeadingWhitespace, 0, 3));
    assert_eq!(error(" CCO"), (SmilesErrorCode::LeadingWhitespace, 0, 1));
    assert_eq!(
        error("\u{a0}\tC"),
        (SmilesErrorCode::LeadingWhitespace, 0, 3)
    );
}

#[test]
fn ring_digits_must_follow_their_atom() {
    assert_eq!(
        error("C(1CC)C1"),
        (SmilesErrorCode::RingClosureWithoutAtom, 2, 3)
    );
    assert_eq!(
        error("C(C)1CC1"),
        (SmilesErrorCode::RingClosureAfterBranch, 4, 5)
    );
    assert_eq!(
        error("C(C)=1CC1"),
        (SmilesErrorCode::RingClosureAfterBranch, 5, 6)
    );
    assert_eq!(
        error("1CC1"),
        (SmilesErrorCode::RingClosureWithoutAtom, 0, 1)
    );
    assert_eq!(
        error("C.1C"),
        (SmilesErrorCode::RingClosureWithoutAtom, 2, 3)
    );
    assert_eq!(error("C%1C"), (SmilesErrorCode::InvalidRingNumber, 1, 3));
    assert_eq!(error("C%"), (SmilesErrorCode::InvalidRingNumber, 1, 2));
    assert_eq!(error("C%(12)"), (SmilesErrorCode::InvalidRingNumber, 1, 2));
}

#[test]
fn reports_the_earliest_opened_unclosed_ring() {
    assert_eq!(error("C1CC2CC3"), (SmilesErrorCode::UnclosedRing, 1, 2));
    assert_eq!(error("C2CC1C"), (SmilesErrorCode::UnclosedRing, 1, 2));
    assert_eq!(error("C1CC1C2"), (SmilesErrorCode::UnclosedRing, 6, 7));
}

#[test]
fn ring_closure_graph_errors_carry_the_closing_position() {
    assert_eq!(error("C11"), (SmilesErrorCode::RingClosureSelf, 2, 3));
    assert_eq!(error("C1C1"), (SmilesErrorCode::DuplicateBond, 3, 4));
}

/// One input per error code; the table must name every code of the application.
#[test]
fn every_error_code_is_reachable_with_an_exact_span() {
    let long = "C".repeat(MAX_SMILES_BYTES + 1);
    let deep = format!("C{}", "(C".repeat(MAX_BRANCH_DEPTH + 1));
    let many = "C".repeat(MAX_SMILES_ITEMS);
    let cases: Vec<(String, SmilesErrorCode, usize, usize)> = vec![
        ("".into(), SmilesErrorCode::EmptyInput, 0, 0),
        (" C".into(), SmilesErrorCode::LeadingWhitespace, 0, 1),
        (
            long,
            SmilesErrorCode::InputTooLong,
            MAX_SMILES_BYTES,
            MAX_SMILES_BYTES + 1,
        ),
        ("C\u{e9}".into(), SmilesErrorCode::UnexpectedCharacter, 1, 3),
        ("CK".into(), SmilesErrorCode::BracketRequired, 1, 2),
        ("CX".into(), SmilesErrorCode::UnknownElement, 1, 2),
        ("C[Cu".into(), SmilesErrorCode::UnterminatedBracket, 1, 4),
        ("C[C!]".into(), SmilesErrorCode::InvalidBracketContent, 3, 4),
        ("[0C]".into(), SmilesErrorCode::InvalidIsotope, 1, 2),
        ("[1234C]".into(), SmilesErrorCode::InvalidIsotope, 1, 5),
        ("[CH12]".into(), SmilesErrorCode::InvalidHydrogenCount, 2, 5),
        ("[C+123]".into(), SmilesErrorCode::InvalidCharge, 2, 6),
        ("[C++2]".into(), SmilesErrorCode::InvalidCharge, 2, 5),
        ("[C+9]".into(), SmilesErrorCode::ChargeOutOfRange, 2, 4),
        ("C%1".into(), SmilesErrorCode::InvalidRingNumber, 1, 3),
        ("1C".into(), SmilesErrorCode::RingClosureWithoutAtom, 0, 1),
        (
            "C(C)1".into(),
            SmilesErrorCode::RingClosureAfterBranch,
            4,
            5,
        ),
        ("C11".into(), SmilesErrorCode::RingClosureSelf, 2, 3),
        ("C=1CC#1".into(), SmilesErrorCode::RingBondMismatch, 6, 7),
        ("C1C1".into(), SmilesErrorCode::DuplicateBond, 3, 4),
        ("C1CC".into(), SmilesErrorCode::UnclosedRing, 1, 2),
        ("CC=".into(), SmilesErrorCode::DanglingBond, 2, 3),
        ("=C".into(), SmilesErrorCode::BondWithoutSource, 0, 1),
        ("C==C".into(), SmilesErrorCode::RepeatedBond, 2, 3),
        ("C..C".into(), SmilesErrorCode::EmptyComponent, 1, 2),
        ("(C)C".into(), SmilesErrorCode::BranchWithoutAtom, 0, 1),
        ("C()C".into(), SmilesErrorCode::EmptyBranch, 1, 3),
        ("CC)".into(), SmilesErrorCode::UnmatchedBranchClose, 2, 3),
        ("C(C".into(), SmilesErrorCode::UnclosedBranch, 1, 2),
        (
            deep,
            SmilesErrorCode::BranchDepthLimit,
            1 + 2 * MAX_BRANCH_DEPTH,
            2 + 2 * MAX_BRANCH_DEPTH,
        ),
        (
            many,
            SmilesErrorCode::SizeLimit,
            MAX_SMILES_ITEMS / 2,
            MAX_SMILES_ITEMS / 2 + 1,
        ),
        ("c".into(), SmilesErrorCode::AromaticAtomOutsideRing, 0, 1),
        ("C/C=C/C".into(), SmilesErrorCode::StereoUnsupported, 1, 2),
        (
            "C[C@H](F)Cl".into(),
            SmilesErrorCode::StereoUnsupported,
            3,
            4,
        ),
        ("[C@@H]".into(), SmilesErrorCode::StereoUnsupported, 2, 4),
        ("C*".into(), SmilesErrorCode::QueryUnsupported, 1, 2),
        ("CC>>CO".into(), SmilesErrorCode::ReactionUnsupported, 2, 3),
        ("[CH3:7]".into(), SmilesErrorCode::AtomMapUnsupported, 4, 6),
        ("C$C".into(), SmilesErrorCode::BondKindUnsupported, 1, 2),
    ];
    let mut seen = HashSet::new();
    for (text, code, start, end) in &cases {
        let shown: String = text.chars().take(24).collect();
        assert_eq!(error(text), (*code, *start, *end), "{shown:?}");
        seen.insert(*code);
    }
    for code in SmilesErrorCode::ALL {
        assert!(
            seen.contains(&code),
            "no test input produces {}",
            code.code()
        );
    }
}

#[test]
fn branch_and_dot_misuse_is_positioned() {
    assert_eq!(error("C(=)C").0, SmilesErrorCode::DanglingBond);
    assert_eq!(error("C(.)C").0, SmilesErrorCode::EmptyComponent);
    assert_eq!(error("C.").0, SmilesErrorCode::EmptyComponent);
    assert_eq!(error(".C").0, SmilesErrorCode::EmptyComponent);
    assert_eq!(error("C.(C)").0, SmilesErrorCode::BranchWithoutAtom);
    assert_eq!(error("C((C))").0, SmilesErrorCode::BranchWithoutAtom);
    assert_eq!(read("C(.C)C").bonds.len(), 1);
}

#[test]
fn spans_are_utf8_offsets_on_char_boundaries() {
    for text in [
        "\u{3b1}",
        "C\u{3b1}",
        "[C\u{1f600}]",
        "C(\u{e9})",
        "[\u{e9}]",
        "C%\u{e9}",
        "[1\u{e9}]",
    ] {
        let (_, start, end) = error(text);
        assert!(
            text.is_char_boundary(start) && text.is_char_boundary(end),
            "{text:?}"
        );
    }
    assert_eq!(
        error("C\u{1f600}"),
        (SmilesErrorCode::UnexpectedCharacter, 1, 5)
    );
}

#[test]
fn never_panics_on_odd_input_and_is_bounded() {
    let big = "(".repeat(1_000_000);
    let nested = format!("C{}", "(C".repeat(5_000));
    for text in [
        "\u{0}",
        "\u{feff}C",
        "C\u{0}C",
        "[",
        "[]",
        "[[",
        "%",
        "%%",
        "((((",
        "))))",
        "..",
        "C..",
        "[C",
        "[CH",
        "[C+",
        "[C+-]",
        "1",
        "9",
        "C0",
        "C%00",
        "C%99",
        "\u{85}",
    ] {
        let _ = SmilesTextCodec.read(text);
    }
    assert_eq!(
        SmilesTextCodec.read(&big).unwrap_err().code,
        SmilesErrorCode::InputTooLong
    );
    assert_eq!(
        SmilesTextCodec.read(&nested).unwrap_err().code,
        SmilesErrorCode::BranchDepthLimit
    );
}

/// A deterministic generator; the seed is part of every failure message.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

#[test]
fn seeded_fuzz_never_panics_and_accepted_input_rewrites() {
    let alphabet: Vec<&str> = vec![
        "C",
        "c",
        "N",
        "n",
        "O",
        "Cl",
        "Br",
        "[",
        "]",
        "(",
        ")",
        "1",
        "2",
        "%",
        "10",
        "=",
        "#",
        "-",
        ":",
        ".",
        "+",
        "H",
        "@",
        "/",
        "\\",
        " ",
        "\u{e9}",
        "[nH]",
        "[O-]",
        "[13C]",
        "*",
        ">",
        "$",
        "\n",
        "\u{1f600}",
    ];
    for seed in 0..4000_u64 {
        let mut rng = Lcg(seed);
        let length = rng.next() % 24;
        let text: String = (0..length)
            .map(|_| alphabet[(rng.next() as usize) % alphabet.len()])
            .collect();
        match SmilesTextCodec.read(&text) {
            Ok(read) => {
                if let Ok(written) = SmilesTextCodec.write(&read.molecule) {
                    let again = SmilesTextCodec.read(&written).unwrap_or_else(|e| {
                        panic!("seed {seed}: {text:?} wrote {written:?}: {e:?}")
                    });
                    assert_eq!(
                        again.molecule.atoms.len(),
                        read.molecule.atoms.len(),
                        "seed {seed}: {text:?}"
                    );
                    assert_eq!(
                        again.molecule.bonds.len(),
                        read.molecule.bonds.len(),
                        "seed {seed}: {text:?}"
                    );
                }
            }
            Err(e) => {
                assert!(
                    e.start <= e.end
                        && e.end <= text.len()
                        && text.is_char_boundary(e.start)
                        && text.is_char_boundary(e.end),
                    "seed {seed}: {text:?} gave span {}..{}",
                    e.start,
                    e.end
                );
            }
        }
    }
}

fn graph(m: &SmilesMolecule) -> Vec<String> {
    let mut edges: Vec<String> = m
        .bonds
        .iter()
        .map(|b| {
            let (a, c) = (b.begin.min(b.end), b.begin.max(b.end));
            format!("{a}-{c}:{:?}", b.kind)
        })
        .collect();
    edges.sort();
    let mut out: Vec<String> = m
        .atoms
        .iter()
        .map(|a| {
            format!(
                "{}{:?}{:?}{}{}",
                a.element.symbol(),
                a.isotope,
                a.hydrogens,
                a.charge,
                a.aromatic
            )
        })
        .collect();
    out.extend(edges);
    out
}

#[test]
fn written_text_reads_back_to_the_same_molecule() {
    for text in [
        "CC(=O)O.[Na+].[Cl-]",
        "C1CC1",
        "c1ccccc1-c1ccccc1",
        "c1ccc2ccccc2c1",
        "[nH]1cccc1",
        "[13CH3-][2H]",
        "C12C3C4C1C5C2C3C45",
        "C(C)(C)(C)C",
        "C1(C)(C)CC1",
        "OC(=O)C#N",
        "[H][H]",
        "C=1CCCCC=1",
        "C1CC2CC1C2",
        "[NH4+]",
    ] {
        let m = read(text);
        let written = SmilesTextCodec.write(&m).unwrap();
        let again = read(&written);
        assert_eq!(graph(&again), graph(&m), "{text} wrote {written}");
        assert_eq!(
            SmilesTextCodec.write(&again).unwrap(),
            written,
            "{text}: writing is a fixpoint"
        );
    }
}

#[test]
fn written_text_is_deterministic_and_uses_small_subtrees_as_branches() {
    let m = read("OC(CCCCC)C");
    assert_eq!(SmilesTextCodec.write(&m).unwrap(), "OC(C)CCCCC");
    let b = read("C1=CC=CC=C1-C");
    assert_eq!(SmilesTextCodec.write(&b).unwrap(), "C1=CC=CC=C1C");
}

#[test]
fn writer_refuses_what_the_reader_would_refuse() {
    // A star of branches deeper than the reader allows cannot be produced: nesting is small.
    let mut text = String::from("C");
    for _ in 0..200 {
        text.push_str("(C)");
    }
    text.push('C');
    // 201 atoms, 201 bonds: within the item limit, but the output must still re-read.
    let m = read(&text);
    let written = SmilesTextCodec.write(&m).unwrap();
    assert_eq!(read(&written).atoms.len(), 202);

    let mut oversized = read("CC");
    let proto = oversized.atoms[0].clone();
    oversized.atoms = vec![proto; MAX_SMILES_ITEMS + 1];
    oversized.bonds.clear();
    assert_eq!(
        SmilesTextCodec.write(&oversized).unwrap_err(),
        SmilesFormatError::SizeLimit
    );

    let mut chain = read("C");
    let proto = chain.atoms[0].clone();
    chain.atoms = vec![proto; 3];
    chain.bonds = vec![chemifree_application::smiles::SmilesBond {
        begin: 0,
        end: 5,
        kind: SmilesBondKind::Single,
    }];
    assert_eq!(
        SmilesTextCodec.write(&chain).unwrap_err(),
        SmilesFormatError::NotWritable
    );
}

#[test]
fn writer_numbers_many_rings_and_reuses_labels() {
    // Twenty separate three-membered rings: closed labels must be reused.
    let mut text = String::new();
    for _ in 0..20 {
        text.push_str("C1CC1");
        text.push('.');
    }
    text.pop();
    let m = read(&text);
    let written = SmilesTextCodec.write(&m).unwrap();
    assert!(
        !written.contains('%'),
        "closed labels are reused: {written}"
    );
    assert_eq!(read(&written).bonds.len(), 60);
}

fn bond(begin: usize, end: usize) -> chemifree_application::smiles::SmilesBond {
    chemifree_application::smiles::SmilesBond {
        begin,
        end,
        kind: SmilesBondKind::Single,
    }
}

#[test]
fn writer_refuses_more_open_ring_closures_than_it_can_number() {
    let proto = read("C").atoms[0].clone();
    // A path of 101 atoms with atom 0 also bonded to every atom from 2 to 100: 99 ring edges.
    let mut molecule = SmilesMolecule {
        atoms: vec![proto.clone(); 101],
        bonds: Vec::new(),
    };
    molecule.bonds.extend((1..101).map(|i| bond(i - 1, i)));
    molecule.bonds.extend((2..101).map(|i| bond(0, i)));
    let written = SmilesTextCodec.write(&molecule).unwrap();
    assert_eq!(read(&written).bonds.len(), molecule.bonds.len());
    // One more ring edge needs a hundredth label.
    let mut molecule = SmilesMolecule {
        atoms: vec![proto; 102],
        bonds: Vec::new(),
    };
    molecule.bonds.extend((1..102).map(|i| bond(i - 1, i)));
    molecule.bonds.extend((2..102).map(|i| bond(0, i)));
    assert_eq!(
        SmilesTextCodec.write(&molecule).unwrap_err(),
        SmilesFormatError::RingLabelLimit
    );
}
