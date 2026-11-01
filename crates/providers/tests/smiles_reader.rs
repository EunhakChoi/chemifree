//! The SMILES reader: grammar decisions (OpenSMILES section noted per group), error codes and
//! spans, and a seeded fuzz proving it never panics.

mod smiles_common;

use chemifree_application::smiles_codec::{ErrorClass, ReadNote, SmilesCodec, SourceSpan};
use chemifree_domain::document::{BondOrder, MAX_CHARGE, MIN_CHARGE};
use chemifree_domain::states::{Hydrogens, Radical};
use chemifree_providers::smiles::{Code, MAX_BRANCH_DEPTH, MAX_COMMANDS, MAX_INPUT_BYTES};
use smiles_common::{CODEC, Rng, read};

/// (input, code, first byte, one past the last byte)
const REJECTED: &[(&str, &str, usize, usize)] = &[
    // Whitespace and emptiness (Grammar: terminator).
    ("", "empty_input", 0, 0),
    (" ", "leading_whitespace", 0, 1),
    (" C", "leading_whitespace", 0, 1),
    ("\tC", "leading_whitespace", 0, 1),
    ("\nC", "leading_whitespace", 0, 1),
    ("\r\n", "leading_whitespace", 0, 1),
    // Unexpected text.
    ("C!", "unexpected_character", 1, 2),
    ("Cé", "unexpected_character", 1, 3),
    ("é", "unexpected_character", 0, 2),
    ("H", "unexpected_character", 0, 1),
    ("Na", "unexpected_character", 1, 2),
    // Brackets (Atoms).
    ("[", "unclosed_bracket", 0, 1),
    ("[C", "unclosed_bracket", 0, 1),
    ("[C ", "unclosed_bracket", 0, 1),
    ("[]", "empty_bracket", 0, 2),
    ("[12]", "empty_bracket", 0, 4),
    ("[Xx]", "unknown_element", 1, 2),
    ("[X]", "unknown_element", 1, 2),
    ("[x]", "unknown_element", 1, 2),
    ("[C+-]", "charge_syntax", 2, 4),
    ("[C+++]", "charge_syntax", 2, 5),
    ("[C+100]", "charge_syntax", 2, 6),
    ("[C-H3]", "bracket_unexpected_character", 3, 4),
    ("[CH10]", "bracket_unexpected_character", 4, 5),
    ("[C?]", "bracket_unexpected_character", 2, 3),
    ("[Cé]", "bracket_unexpected_character", 2, 4),
    ("[1234C]", "isotope_too_long", 1, 5),
    // Branches.
    ("C(", "unclosed_branch", 1, 2),
    ("C(C", "unclosed_branch", 1, 2),
    ("C(C(C)", "unclosed_branch", 1, 2),
    ("C)", "unmatched_close_paren", 1, 2),
    ("C()", "empty_branch", 1, 3),
    ("(C)", "branch_without_atom", 0, 1),
    ("C.(C)", "branch_without_atom", 2, 3),
    ("C((C))", "branch_without_atom", 2, 3),
    // Bonds: a symbol must be followed by an atom or ring digit.
    ("C=", "dangling_bond", 1, 2),
    ("C=(O)", "dangling_bond", 1, 2),
    ("C(=O=)", "dangling_bond", 4, 5),
    ("C(=)C", "dangling_bond", 2, 3),
    ("C=.C", "dangling_bond", 1, 2),
    ("C#", "dangling_bond", 1, 2),
    ("C-", "dangling_bond", 1, 2),
    ("C==C", "consecutive_bonds", 2, 3),
    ("=C", "bond_without_atom", 0, 1),
    (".=C", "misplaced_dot", 0, 1),
    ("C.=C", "bond_without_atom", 2, 3),
    // Dots.
    (".C", "misplaced_dot", 0, 1),
    ("C..C", "misplaced_dot", 2, 3),
    ("C.", "dangling_dot", 1, 2),
    ("C(C.)C", "dangling_dot", 3, 4),
    // Ring digits sit right after their atom (Ring Closures).
    ("1C", "ring_digit_without_atom", 0, 1),
    ("C.1C", "ring_digit_without_atom", 2, 3),
    ("C(1CC)C", "ring_digit_without_atom", 2, 3),
    ("C(=1CC)C", "ring_digit_without_atom", 3, 4),
    ("C(C)1", "ring_digit_after_branch", 4, 5),
    ("C(C)=1", "ring_digit_after_branch", 5, 6),
    ("C(C)%12", "ring_digit_after_branch", 4, 5),
    ("C%", "ring_label_syntax", 1, 2),
    ("C%1", "ring_label_syntax", 1, 3),
    ("C%a", "ring_label_syntax", 1, 2),
    ("C%(", "ring_label_syntax", 1, 3),
    ("C%()", "ring_label_syntax", 1, 3),
    ("C1", "unclosed_ring", 1, 2),
    ("C1CC2CC", "unclosed_ring", 1, 2),
    ("C%12CC", "unclosed_ring", 1, 4),
    ("C=1CC", "unclosed_ring", 1, 3),
    ("C11", "ring_self_bond", 2, 3),
    ("C1C1", "duplicate_bond", 3, 4),
    ("C-1CC=1", "ring_bond_conflict", 5, 6),
    // Valid SMILES the drawing cannot hold.
    ("C[C@H](O)N", "stereo_unsupported", 3, 4),
    ("C[C@@H](O)N", "stereo_unsupported", 3, 4),
    ("C/C=C/C", "stereo_unsupported", 1, 2),
    ("C\\C=C\\C", "stereo_unsupported", 1, 2),
    ("F/C=C/F", "stereo_unsupported", 1, 2),
    ("[C@]", "stereo_unsupported", 2, 3),
    ("*", "wildcard_unsupported", 0, 1),
    ("C*", "wildcard_unsupported", 1, 2),
    ("[*]", "wildcard_unsupported", 1, 2),
    ("C$C", "quadruple_bond_unsupported", 1, 2),
    ("[CH4:1]", "atom_class_unsupported", 4, 6),
    ("[C:12]", "atom_class_unsupported", 2, 5),
    ("C%(123)C%(123)", "extended_ring_label_unsupported", 1, 7),
    ("CC>>CC", "reaction_unsupported", 2, 3),
    ("[Fe+9]", "charge_out_of_range", 3, 5),
    ("[Fe+15]", "charge_out_of_range", 3, 6),
    ("[O-99]", "charge_out_of_range", 2, 5),
    ("[0S]", "isotope_zero_unsupported", 1, 2),
    ("[000C]", "isotope_zero_unsupported", 1, 4),
    // Aromatic atoms and bonds (Aromaticity).
    ("c", "aromatic_atom_outside_ring", 0, 1),
    ("c:c", "aromatic_bridge_unsupported", 1, 2),
    ("c1ccccc1:c1ccccc1", "aromatic_bridge_unsupported", 8, 9),
    ("c1ccccc1c", "aromatic_atom_outside_ring", 8, 9),
    ("C1CCn1", "aromatic_atom_outside_ring", 4, 5),
    ("c1ccccc1.c1cccc1", "aromatic_system_unsupported", 9, 10),
    ("C.c1cccc1", "aromatic_system_unsupported", 2, 3),
    ("c1cccc1C", "aromatic_system_unsupported", 0, 1),
    ("[CH2]1cccc1", "aromatic_system_unsupported", 0, 5),
    // A radical-looking aromatic atom fails the aromaticity model before any radical is read.
    ("c1cc[c]cc1", "aromatic_system_unsupported", 0, 1),
];

#[test]
fn rejects_with_code_and_position() {
    for &(input, code, start, end) in REJECTED {
        let error = read(input).expect_err(input);
        assert_eq!(error.code, code, "{input:?}: {}", error.message);
        assert_eq!(
            error.span,
            Some(SourceSpan { start, end }),
            "{input:?} {code}"
        );
        // Every span is on char boundaries of the input.
        assert!(
            input.is_char_boundary(start) && input.is_char_boundary(end),
            "{input:?} span {start}..{end} splits a character"
        );
    }
}

#[test]
fn classes_follow_the_codes() {
    for &(input, code, ..) in REJECTED {
        let error = read(input).unwrap_err();
        let expected = Code::ALL
            .iter()
            .find(|candidate| candidate.as_str() == code)
            .expect(code)
            .class();
        assert_eq!(error.class, expected, "{input:?}");
    }
    // Valid OpenSMILES the drawing cannot hold is Unsupported; malformed text stays Syntax.
    for input in [
        "C%(123)C%(123)",
        "[Fe+9]",
        "[0S]",
        "C[C@H](O)N",
        "*",
        "[C:1]",
        "C$C",
    ] {
        assert_eq!(
            read(input).unwrap_err().class,
            ErrorClass::Unsupported,
            "{input}"
        );
    }
    for input in ["[Fe+100]", "C%1", "C%(", "[C+-]", "C(C)1"] {
        assert_eq!(
            read(input).unwrap_err().class,
            ErrorClass::Syntax,
            "{input}"
        );
    }
}

#[test]
fn the_charge_limits_are_the_domain_constants() {
    for charge in MIN_CHARGE..=MAX_CHARGE {
        let text = match charge {
            0 => "[Fe+0]".to_owned(),
            charge if charge > 0 => format!("[Fe+{charge}]"),
            charge => format!("[Fe-{}]", -i32::from(charge)),
        };
        assert_eq!(read(&text).unwrap().atoms[0].atom.charge, charge, "{text}");
    }
    for text in [
        format!("[Fe+{}]", i32::from(MAX_CHARGE) + 1),
        format!("[Fe-{}]", -i32::from(MIN_CHARGE) + 1),
    ] {
        assert_eq!(
            read(&text).unwrap_err().code,
            "charge_out_of_range",
            "{text}"
        );
    }
    // Zero charges, the deprecated doubled signs and sign-only spellings (Charge).
    assert_eq!(read("[Fe-0]").unwrap().atoms[0].atom.charge, 0);
    assert_eq!(read("[Fe++]").unwrap().atoms[0].atom.charge, 2);
    assert_eq!(read("[O--]").unwrap().atoms[0].atom.charge, -2);
    assert_eq!(read("[Na+]").unwrap().atoms[0].atom.charge, 1);
    assert_eq!(read("[Cl-1]").unwrap().atoms[0].atom.charge, -1);
}

#[test]
fn isotopes_allow_leading_zeros_up_to_three_digits() {
    for (text, mass) in [
        ("[2H]", 2),
        ("[02H]", 2),
        ("[002H]", 2),
        ("[13C]", 13),
        ("[999Fe]", 999),
    ] {
        assert_eq!(
            read(text).unwrap().atoms[0].atom.isotope,
            Some(mass),
            "{text}"
        );
    }
    assert_eq!(read("[C]").unwrap().atoms[0].atom.isotope, None);
}

#[test]
fn whitespace_ends_the_smiles_and_trailing_text_is_reported() {
    for terminator in [" ", "\t", "\n", "\r", "\r\n", "  \t"] {
        let text = format!("CCO{terminator}");
        let read = read(&text).unwrap();
        assert_eq!(read.atoms.len(), 3);
        assert!(read.notes.is_empty(), "trailing whitespace only: {text:?}");
    }
    let read = read("CCO ethanol, a name").unwrap();
    assert_eq!(read.atoms.len(), 3);
    assert_eq!(
        read.notes,
        vec![ReadNote::IgnoredText(SourceSpan { start: 4, end: 19 })]
    );
    let multibyte = smiles_common::read("C\t名前").unwrap();
    assert_eq!(
        multibyte.notes,
        vec![ReadNote::IgnoredText(SourceSpan { start: 2, end: 8 })]
    );
}

#[test]
fn ring_closures_use_digits_and_percent_labels() {
    let cases: &[(&str, usize, usize)] = &[
        ("C1CCCCC1", 6, 6),
        ("C%25CCCCC%25", 6, 6),
        ("C0CCCCC0", 6, 6),
        ("C1CCCCC%01", 6, 6),
        ("C%00CCCCC%00", 6, 6),
        ("C12(CCCCC1)CCCCC2", 11, 12),
        ("C1CCCCC1C1CCCCC1", 12, 13),
        // %123 is %12 then 3 (Ring Closures).
        ("C%123CCCC3C%12", 6, 7),
        ("C1.C1", 2, 1),
        ("C=1CCCCC1", 6, 6),
        ("C1CCCCC=1", 6, 6),
        ("C=1CCCCC=1", 6, 6),
        ("C1=2CCC1CC2", 6, 7),
    ];
    for &(text, atoms, bonds) in cases {
        let read = read(text).expect(text);
        assert_eq!(
            (read.atoms.len(), read.bonds.len()),
            (atoms, bonds),
            "{text}"
        );
    }
    // A label closed at an atom may be opened again by the same atom.
    let reuse = read("C11CC1CC1").unwrap_err();
    assert_eq!(reuse.code, "ring_self_bond");
    let again = read("C1CC11CC1").unwrap();
    assert_eq!(
        (again.atoms.len(), again.bonds.len()),
        (5, 6),
        "{:?}",
        again.bonds
    );
    // The ring bond symbol sets the order from either end.
    let from_open = read("C=1CCCCC1").unwrap();
    let from_close = read("C1CCCCC=1").unwrap();
    for read in [from_open, from_close] {
        assert_eq!(
            read.bonds
                .iter()
                .filter(|b| b.order == BondOrder::Double)
                .count(),
            1
        );
    }
    // The earliest-opened unclosed ring is reported.
    let error = read("C2C1CC").unwrap_err();
    assert_eq!(
        (error.code, error.span),
        ("unclosed_ring", Some(SourceSpan { start: 1, end: 2 }))
    );
}

#[test]
fn dots_and_branches_make_parts_and_bonds() {
    let read = read("Oc1ccccc1.NCCO").unwrap();
    assert_eq!(read.parts, 2);
    let nested = smiles_common::read("c1cc(O.NCCO)ccc1").unwrap();
    assert_eq!(nested.parts, 2);
    assert_eq!(nested.bonds.len(), 6 + 1 + 3);
    let dot_in_branch = smiles_common::read("Oc1cc(.NCCO)ccc1").unwrap();
    assert_eq!(dot_in_branch.bonds.len(), 1 + 6 + 3);
    let branch_bond = smiles_common::read("CC(=O)C#N").unwrap();
    let orders: Vec<_> = branch_bond.bonds.iter().map(|b| b.order.clone()).collect();
    assert_eq!(
        orders,
        vec![
            BondOrder::Single,
            BondOrder::Double,
            BondOrder::Single,
            BondOrder::Triple
        ]
    );
}

#[test]
fn aromatic_bonds_are_ring_bonds_between_aromatic_atoms() {
    let biphenyl = read("c1ccccc1c1ccccc1").unwrap();
    let aromatic = biphenyl
        .bonds
        .iter()
        .filter(|b| b.order == BondOrder::Aromatic)
        .count();
    assert_eq!(aromatic, 12);
    assert_eq!(biphenyl.bonds.len(), 13);
    assert!(biphenyl.bonds.iter().any(|b| b.order == BondOrder::Single));
    // An explicit single bond between aromatic atoms in a ring stays single.
    let explicit = read("c1ccccc1-c1ccccc1").unwrap();
    assert_eq!(
        explicit
            .bonds
            .iter()
            .filter(|b| b.order == BondOrder::Aromatic)
            .count(),
        12
    );
    // An aromatic atom bonded to an aliphatic one: single.
    let toluene = read("Cc1ccccc1").unwrap();
    assert_eq!(toluene.bonds[0].order, BondOrder::Single);
    // An explicit colon inside a ring is accepted.
    assert_eq!(
        read("c1:c:c:c:c:c:1")
            .unwrap()
            .bonds
            .iter()
            .filter(|b| b.order == BondOrder::Aromatic)
            .count(),
        6
    );
}

#[test]
fn bracket_atoms_state_hydrogens_and_get_radicals_from_the_shared_model() {
    let radical = |text: &str| {
        let read = read(text).unwrap();
        (read.atoms[0].atom.radical, read.atoms[0].atom.hydrogens)
    };
    assert_eq!(radical("[CH4]"), (Radical::None, Hydrogens::Specified(4)));
    assert_eq!(
        radical("[CH3]"),
        (Radical::Doublet, Hydrogens::Specified(3))
    );
    assert_eq!(radical("[OH]"), (Radical::Doublet, Hydrogens::Specified(1)));
    assert_eq!(radical("[H]"), (Radical::Doublet, Hydrogens::Specified(0)));
    assert_eq!(radical("[NH4+]"), (Radical::None, Hydrogens::Specified(4)));
    assert_eq!(radical("C"), (Radical::None, Hydrogens::Inferred));
    // No single valence-consistent state: unknown, and reported, never silently none.
    for (text, reason) in [
        ("[C]", "NoState"),
        ("[CH]", "NoState"),
        ("[N]", "NoState"),
        ("[CH2]", "Ambiguous"),
        ("[NH]", "Ambiguous"),
        ("[Na+]", "OutsideModel"),
        ("[Fe]", "OutsideModel"),
        ("[SiH4]", "OutsideModel"),
    ] {
        let read = read(text).unwrap();
        assert_eq!(read.atoms[0].atom.radical, Radical::Unknown, "{text}");
        assert!(
            read.notes
                .iter()
                .any(|note| format!("{note:?}").contains(reason)),
            "{text}: {:?}",
            read.notes
        );
    }
    // A radical state follows the bonds too: methyl on a carbon.
    assert_eq!(
        smiles_common::read("C[CH2]").unwrap().atoms[1].atom.radical,
        Radical::Doublet
    );
}

#[test]
fn limits_are_reported_as_limits() {
    let long = "C".repeat(MAX_INPUT_BYTES + 1);
    let error = read(&long).unwrap_err();
    assert_eq!(
        (error.code, error.class),
        ("input_too_long", ErrorClass::Limit)
    );
    assert_eq!(
        error.span,
        Some(SourceSpan {
            start: MAX_INPUT_BYTES,
            end: MAX_INPUT_BYTES + 1
        })
    );
    // Multibyte text at the limit: the span starts on a character boundary.
    for pad in 0..4 {
        let text = format!("{}{}", "C".repeat(MAX_INPUT_BYTES - pad), "名".repeat(3));
        let error = read(&text).unwrap_err();
        let span = error.span.unwrap();
        assert_eq!(error.code, "input_too_long");
        assert!(
            text.is_char_boundary(span.start) && text.is_char_boundary(span.end),
            "pad {pad}"
        );
        assert!(
            span.start <= MAX_INPUT_BYTES && span.end == text.len(),
            "pad {pad} {span:?}"
        );
    }
    let exactly = "C".repeat(MAX_INPUT_BYTES);
    assert_ne!(
        read(&exactly).map(|_| ()).err().map(|e| e.code),
        Some("input_too_long")
    );
    // Atoms plus bonds are bounded by one edit's command limit while parsing.
    let chain = "C".repeat(MAX_COMMANDS / 2 + 1);
    let error = read(&chain).unwrap_err();
    assert_eq!(
        (error.code, error.class),
        ("command_limit", ErrorClass::Limit)
    );
    assert_eq!(
        error.span.unwrap().start,
        MAX_COMMANDS / 2,
        "the 251st atom is the first past the limit"
    );
    assert!(read(&"C".repeat(MAX_COMMANDS / 2)).is_ok());
    let deep = format!(
        "{}C{}",
        "C(".repeat(MAX_BRANCH_DEPTH + 1),
        ")".repeat(MAX_BRANCH_DEPTH + 1)
    );
    assert_eq!(read(&deep).unwrap_err().code, "branch_depth_limit");
    let ok = format!(
        "{}C{}",
        "C(".repeat(MAX_BRANCH_DEPTH),
        ")".repeat(MAX_BRANCH_DEPTH)
    );
    assert!(read(&ok).is_ok());
}

#[test]
fn every_code_has_a_unique_message_and_the_table_agrees_with_the_tests() {
    let mut codes = std::collections::BTreeSet::new();
    let mut messages = std::collections::BTreeSet::new();
    for code in Code::ALL {
        assert!(!code.message().is_empty(), "{}", code.as_str());
        assert!(
            codes.insert(code.as_str()),
            "duplicate code {}",
            code.as_str()
        );
        assert!(
            messages.insert(code.message()),
            "duplicate message for {}",
            code.as_str()
        );
    }
    // Every code the reader tests expect exists in the table.
    for &(input, code, ..) in REJECTED {
        assert!(
            codes.contains(code),
            "{input:?} expects unknown code {code}"
        );
    }
    // And the reader emits the user message of its code.
    for &(input, code, ..) in REJECTED {
        let error = read(input).unwrap_err();
        let entry = Code::ALL.iter().find(|c| c.as_str() == code).unwrap();
        assert_eq!(error.message, entry.message(), "{input:?}");
    }
    // Reader-side codes are all exercised here; writer-side codes are in the writer tests.
    let exercised: std::collections::BTreeSet<_> = REJECTED.iter().map(|r| r.1).collect();
    for code in Code::ALL {
        let reader_side = matches!(code.class(), ErrorClass::Syntax | ErrorClass::Unsupported);
        if reader_side {
            assert!(
                exercised.contains(code.as_str()),
                "{} has no reader test",
                code.as_str()
            );
        }
    }
}

#[test]
fn never_panics_on_edge_inputs() {
    let mut inputs: Vec<String> = vec![
        String::new(),
        " ".into(),
        "\0".into(),
        "\u{feff}C".into(),
        "C\0".into(),
        "[".repeat(5000),
        "(".repeat(5000),
        ")".repeat(5000),
        "%".repeat(5000),
        "C".repeat(100_000),
        "名".repeat(5000),
        "[C@@H]".repeat(10),
        "C1".repeat(500),
        "%99".into(),
        "C%99C%99".into(),
        "\u{10ffff}".into(),
        "C=".repeat(2000),
        "-".repeat(10),
        "[".into(),
    ];
    inputs.push(format!("[{}]", "9".repeat(5000)));
    inputs.push(format!("C{}", "1".repeat(5000)));
    for input in &inputs {
        let _ = CODEC.read(input);
    }
}

#[test]
fn seeded_fuzz_and_mutation_never_panic_and_spans_stay_on_boundaries() {
    let alphabet: Vec<&str> = vec![
        "C", "c", "N", "n", "O", "o", "S", "s", "Cl", "Br", "[", "]", "(", ")", ".", "=", "#", "-",
        ":", "$", "/", "\\", "@", "+", "H", "1", "2", "9", "%", "%12", "%(", "[NH4+]", "[nH]",
        "[2H]", " ", "\t", "名", "é", "*", ">", "[Fe+3]", "[C-]", "0", "1",
    ];
    let seeds = [
        "CCO",
        "c1ccccc1",
        "C1=CC=CC=C1",
        "[Na+].[Cl-]",
        "CC(=O)Oc1ccccc1C(=O)O",
        "C12(CCCCC1)CCCCC2",
    ];
    let mut rng = Rng(0x5eed_0001);
    let mut accepted = 0;
    for round in 0..4000 {
        let mut text = if round % 2 == 0 {
            String::new()
        } else {
            seeds[rng.below(seeds.len())].to_owned()
        };
        for _ in 0..1 + rng.below(8) {
            let token = alphabet[rng.below(alphabet.len())];
            // Insert at a character boundary, delete a character, or append.
            let boundaries: Vec<usize> = text
                .char_indices()
                .map(|(i, _)| i)
                .chain([text.len()])
                .collect();
            let at = boundaries[rng.below(boundaries.len())];
            match rng.below(3) {
                0 => text.insert_str(at, token),
                1 if at < text.len() => {
                    let end = boundaries.iter().copied().find(|&b| b > at).unwrap();
                    text.replace_range(at..end, "");
                }
                _ => text.push_str(token),
            }
        }
        match CODEC.read(&text) {
            Ok(read) => {
                accepted += 1;
                assert!(read.atoms.len() + read.bonds.len() <= MAX_COMMANDS);
            }
            Err(error) => {
                if let Some(span) = error.span {
                    assert!(
                        span.start <= span.end && span.end <= text.len(),
                        "{text:?} {span:?}"
                    );
                    assert!(
                        text.is_char_boundary(span.start) && text.is_char_boundary(span.end),
                        "{text:?} {span:?} {}",
                        error.code
                    );
                }
            }
        }
    }
    assert!(
        accepted > 100,
        "the fuzz must also reach accepted inputs, got {accepted}"
    );
}
