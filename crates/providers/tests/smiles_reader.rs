//! The SMILES reader: grammar, error positions and codes, limits, and the semantics it takes
//! from the shared valence and aromaticity models.

use chemifree_application::smiles::{SmilesCode, SmilesCodec, SmilesRead};
use chemifree_domain::document::{BondOrder, MAX_CHARGE, MIN_CHARGE};
use chemifree_domain::states::{Hydrogens, Radical};
use chemifree_providers::{MAX_BRANCH_DEPTH, MAX_INPUT_BYTES, OpenSmilesCodec};

fn read(text: &str) -> SmilesRead {
    OpenSmilesCodec
        .read(text)
        .unwrap_or_else(|error| panic!("{text:?}: {error} {error:?}"))
}

/// The refusal as `(code, offset, length)`.
fn refused(text: &str) -> (SmilesCode, usize, usize) {
    let error = OpenSmilesCodec
        .read(text)
        .expect_err(&format!("{text:?} should be refused"));
    let span = error
        .span
        .unwrap_or_else(|| panic!("{text:?}: {:?} has no span", error.code));
    (error.code, span.offset, span.length)
}

fn orders(read: &SmilesRead) -> Vec<(usize, usize, BondOrder)> {
    read.bonds
        .iter()
        .map(|bond| (bond.begin, bond.end, bond.order.clone()))
        .collect()
}

#[test]
fn chains_branches_rings_and_dots_read() {
    for (text, atoms, bonds) in [
        ("CCO", 3, 2),
        ("CC(C)C", 4, 3),
        ("CC(C)(C)C", 5, 4),
        ("C1CCCCC1", 6, 6),
        ("C%12CCCCC%12", 6, 6),
        ("C0CCCCC0", 6, 6),
        ("C12(CCCCC1)CCCCC2", 11, 12),
        ("C1CC1C1CC1", 6, 7),
        ("C[Na]", 2, 1),
        ("[Na+].[Cl-]", 2, 0),
        ("C(.C)C", 3, 1),
        ("C.1CC", 0, 0),
    ] {
        if text == "C.1CC" {
            assert_eq!(refused(text), (SmilesCode::RingDigitAfterDot, 2, 1));
            continue;
        }
        let parsed = read(text);
        assert_eq!((parsed.atoms.len(), parsed.bonds.len()), (atoms, bonds), "{text}");
    }
}

#[test]
fn ring_labels_are_numbers_so_percent_zero_one_is_label_one() {
    // OpenSMILES "Rings": the digits are a number, and two rnums match if the numbers match.
    assert_eq!(read("C1CCCCC%01").bonds.len(), 6);
    assert_eq!(read("C%00CCCCC0").bonds.len(), 6);
}

#[test]
fn a_label_that_closes_at_an_atom_can_open_again_there() {
    let parsed = read("C1CC11CC1");
    assert_eq!(parsed.atoms.len(), 5);
    assert_eq!(parsed.bonds.len(), 6);
    let bonds = orders(&parsed);
    assert!(bonds.iter().any(|(a, b, _)| (*a, *b) == (0, 2)), "{bonds:?}");
    assert!(bonds.iter().any(|(a, b, _)| (*a, *b) == (2, 4)), "{bonds:?}");
}

#[test]
fn ring_bond_symbols_may_stand_at_either_or_both_ends() {
    for text in ["C=1CCCCC1", "C1CCCCC=1", "C=1CCCCC=1"] {
        let parsed = read(text);
        let ring = parsed
            .bonds
            .iter()
            .find(|bond| bond.begin == 0 && bond.end == 5)
            .unwrap_or_else(|| panic!("{text}: no ring bond"));
        assert_eq!(ring.order, BondOrder::Double, "{text}");
    }
    assert_eq!(refused("C-1CCCCC=1"), (SmilesCode::RingBondMismatch, 9, 1));
}

#[test]
fn spans_of_syntax_errors_are_exact() {
    for (text, expected) in [
        ("", (SmilesCode::EmptyInput, 0, 0)),
        (" CCO", (SmilesCode::LeadingWhitespace, 0, 1)),
        ("\tC", (SmilesCode::LeadingWhitespace, 0, 1)),
        ("\nC", (SmilesCode::LeadingWhitespace, 0, 1)),
        ("\rC", (SmilesCode::LeadingWhitespace, 0, 1)),
        ("C(", (SmilesCode::UnclosedBranch, 1, 1)),
        ("C(C", (SmilesCode::UnclosedBranch, 1, 1)),
        ("C(C(C)", (SmilesCode::UnclosedBranch, 1, 1)),
        ("C)", (SmilesCode::UnmatchedCloseBranch, 1, 1)),
        ("()", (SmilesCode::BranchWithoutAtom, 0, 1)),
        ("C((C)C)", (SmilesCode::BranchWithoutAtom, 2, 1)),
        ("C.(C)", (SmilesCode::BranchWithoutAtom, 2, 1)),
        ("C()", (SmilesCode::EmptyBranch, 1, 2)),
        ("C=(O)", (SmilesCode::DanglingBond, 1, 1)),
        ("C(=)C", (SmilesCode::DanglingBond, 2, 1)),
        ("C=", (SmilesCode::DanglingBond, 1, 1)),
        ("C=.C", (SmilesCode::DanglingBond, 1, 1)),
        ("C#", (SmilesCode::DanglingBond, 1, 1)),
        ("C.", (SmilesCode::DanglingDot, 1, 1)),
        ("C..C", (SmilesCode::DanglingDot, 1, 1)),
        ("C(.)", (SmilesCode::DanglingDot, 2, 1)),
        (".C", (SmilesCode::DotWithoutAtom, 0, 1)),
        ("=C", (SmilesCode::BondWithoutAtom, 0, 1)),
        ("C.=C", (SmilesCode::BondWithoutAtom, 2, 1)),
        ("C==C", (SmilesCode::RepeatedBondSymbol, 2, 1)),
        ("1C", (SmilesCode::RingDigitAtStart, 0, 1)),
        ("C(1CC)", (SmilesCode::RingDigitAfterBranchOpen, 2, 1)),
        ("C(C)1", (SmilesCode::RingDigitAfterBranchClose, 4, 1)),
        ("C.1CC", (SmilesCode::RingDigitAfterDot, 2, 1)),
        ("C(=1C)", (SmilesCode::RingDigitAfterDetachedBond, 3, 1)),
        ("C(C)=1", (SmilesCode::RingDigitAfterDetachedBond, 5, 1)),
        ("C1CC", (SmilesCode::UnclosedRing, 1, 1)),
        ("C11", (SmilesCode::RingSelfBond, 2, 1)),
        ("C1C1", (SmilesCode::DuplicateBond, 3, 1)),
        ("C12CCCCC12", (SmilesCode::DuplicateBond, 9, 1)),
        ("C%1C", (SmilesCode::InvalidRingLabel, 1, 2)),
        ("C%C", (SmilesCode::InvalidRingLabel, 1, 1)),
        ("C%(x", (SmilesCode::InvalidRingLabel, 1, 2)),
        ("H", (SmilesCode::BracketRequired, 0, 1)),
        ("X", (SmilesCode::BracketRequired, 0, 1)),
        ("Na", (SmilesCode::UnexpectedCharacter, 1, 1)),
        ("Cé", (SmilesCode::UnexpectedCharacter, 1, 2)),
        ("é", (SmilesCode::UnexpectedCharacter, 0, 2)),
        ("C\u{1F600}", (SmilesCode::UnexpectedCharacter, 1, 4)),
        ("C+", (SmilesCode::UnexpectedCharacter, 1, 1)),
        ("[", (SmilesCode::UnclosedBracket, 0, 1)),
        ("[C", (SmilesCode::UnclosedBracket, 0, 2)),
        ("[C ", (SmilesCode::UnclosedBracket, 0, 2)),
        ("[]", (SmilesCode::MissingElementSymbol, 1, 1)),
        ("[Xx]", (SmilesCode::UnknownElement, 1, 1)),
        ("[HH]", (SmilesCode::HydrogenCountOnHydrogen, 2, 1)),
        ("[CH2+x]", (SmilesCode::InvalidBracketContent, 5, 1)),
        ("[C+H]", (SmilesCode::InvalidBracketContent, 3, 1)),
        ("[C:]", (SmilesCode::InvalidBracketContent, 3, 1)),
        ("[C+100]", (SmilesCode::InvalidBracketContent, 5, 1)),
    ] {
        assert_eq!(refused(text), expected, "{text:?}");
    }
}

#[test]
fn the_earliest_opened_ring_is_the_one_reported() {
    // Label 2 opened before label 1, so label order is not the order that matters.
    assert_eq!(refused("C2CC1CC"), (SmilesCode::UnclosedRing, 1, 1));
    assert_eq!(refused("C1CC2CC"), (SmilesCode::UnclosedRing, 1, 1));
    assert_eq!(refused("C1CC1C2CC3C"), (SmilesCode::UnclosedRing, 6, 1));
}

#[test]
fn valid_but_unsupported_text_is_unsupported_not_syntax() {
    for (text, code, offset, length) in [
        ("[C@H](F)Cl", SmilesCode::StereoUnsupported, 2, 1),
        ("[C@@H](F)Cl", SmilesCode::StereoUnsupported, 2, 1),
        ("F/C=C/F", SmilesCode::StereoUnsupported, 1, 1),
        ("F\\C=C\\F", SmilesCode::StereoUnsupported, 1, 1),
        ("C@C", SmilesCode::StereoUnsupported, 1, 1),
        ("*", SmilesCode::WildcardUnsupported, 0, 1),
        ("[*]", SmilesCode::WildcardUnsupported, 1, 1),
        ("C$C", SmilesCode::QuadrupleBondUnsupported, 1, 1),
        ("C%(123)CC%(123)", SmilesCode::RingLabelExtendedUnsupported, 1, 6),
        ("[C+9]", SmilesCode::ChargeOutOfRange, 2, 2),
        ("[C-9]", SmilesCode::ChargeOutOfRange, 2, 2),
        ("[C+15]", SmilesCode::ChargeOutOfRange, 2, 3),
        ("[Fe-15]", SmilesCode::ChargeOutOfRange, 3, 3),
        ("[Fe+20]", SmilesCode::ChargeOutOfRange, 3, 3),
        ("[0C]", SmilesCode::IsotopeZero, 1, 1),
        ("[000C]", SmilesCode::IsotopeZero, 1, 3),
        ("[1000C]", SmilesCode::IsotopeOutOfRange, 1, 4),
    ] {
        let (found, found_offset, found_length) = refused(text);
        assert_eq!(found, code, "{text}");
        assert_eq!((found_offset, found_length), (offset, length), "{text}");
        assert_eq!(found.class().as_str(), "unsupported", "{text}");
    }
}

#[test]
fn the_charge_limits_are_the_domains() {
    for charge in MIN_CHARGE..=MAX_CHARGE {
        let text = if charge < 0 {
            format!("[Fe-{}]", charge.unsigned_abs())
        } else {
            format!("[Fe+{charge}]")
        };
        assert_eq!(read(&text).atoms[0].atom.charge, charge, "{text}");
    }
    let above = format!("[Fe+{}]", MAX_CHARGE + 1);
    assert_eq!(refused(&above).0, SmilesCode::ChargeOutOfRange);
    let below = format!("[Fe-{}]", MIN_CHARGE.unsigned_abs() + 1);
    assert_eq!(refused(&below).0, SmilesCode::ChargeOutOfRange);
}

#[test]
fn charges_zero_and_repeated_signs_read() {
    for (text, charge) in [
        ("[C+0]", 0),
        ("[C-0]", 0),
        ("[Cu++]", 2),
        ("[O--]", -2),
        ("[Cu+2]", 2),
        ("[OH-1]", -1),
        ("[OH-]", -1),
        ("[Na+]", 1),
        ("[Fe+08]", 8),
    ] {
        assert_eq!(read(text).atoms[0].atom.charge, charge, "{text}");
    }
}

#[test]
fn isotopes_allow_leading_zeros() {
    for text in ["[2H]", "[02H]", "[002H]", "[0002H]"] {
        assert_eq!(read(text).atoms[0].atom.isotope, Some(2), "{text}");
    }
    assert_eq!(read("[999C]").atoms[0].atom.isotope, Some(999));
    assert_eq!(read("[13CH4]").atoms[0].atom.isotope, Some(13));
}

#[test]
fn hydrogen_and_radical_state_come_from_the_valence_model() {
    let methyl = read("[CH3]");
    assert_eq!(methyl.atoms[0].atom.hydrogens, Hydrogens::Specified(3));
    assert_eq!(methyl.atoms[0].atom.radical, Radical::Doublet);
    assert!(methyl.losses.is_empty());
    assert_eq!(read("[OH]").atoms[0].atom.radical, Radical::Doublet);
    assert_eq!(read("[CH4]").atoms[0].atom.radical, Radical::None);
    assert_eq!(read("[NH4+]").atoms[0].atom.radical, Radical::None);
    assert_eq!(read("[Cl-]").atoms[0].atom.radical, Radical::None);
    assert_eq!(read("[H][H]").atoms[0].atom.radical, Radical::None);
    // A bare atom has inferred hydrogens and no radical.
    assert_eq!(read("C").atoms[0].atom.hydrogens, Hydrogens::Inferred);
}

#[test]
fn a_bracket_atom_without_one_consistent_radical_is_unknown_with_a_loss() {
    for text in ["[C]", "[CH]", "[CH2]", "[N]", "[CH5]", "[Na+]", "[Fe]", "[S]"] {
        let parsed = read(text);
        assert_eq!(parsed.atoms[0].atom.radical, Radical::Unknown, "{text}");
        let loss = parsed
            .losses
            .iter()
            .find(|loss| loss.feature == "radical_state")
            .unwrap_or_else(|| panic!("{text}: no radical_state loss"));
        assert_eq!(loss.source_path, format!("source[0..={}]", text.len() - 1));
    }
}

#[test]
fn atom_classes_are_dropped_with_a_loss() {
    let parsed = read("[CH4:5]");
    assert_eq!(parsed.atoms.len(), 1);
    assert!(parsed.losses.iter().any(|loss| loss.feature == "atom_class"));
    assert_eq!(read("[NH4+:005]").atoms[0].atom.charge, 1);
}

#[test]
fn whitespace_ends_the_string_and_later_text_is_reported() {
    for terminator in [" ", "\t", "\n", "\r", "\r\n", "  \t"] {
        let text = format!("CCO{terminator}ethanol");
        let parsed = read(&text);
        assert_eq!(parsed.atoms.len(), 3, "{text:?}");
        let span = parsed.ignored_text.expect("ignored text is reported");
        assert_eq!(&text[span.offset..span.offset + span.length], "ethanol");
        let loss = parsed
            .losses
            .iter()
            .find(|loss| loss.feature == "ignored_text")
            .expect("an ignored_text loss");
        assert_eq!(
            loss.source_path,
            format!("source[{}]", span.describe()),
            "the path names the last byte"
        );
    }
    let two_lines = read("CCO\nCC");
    assert_eq!(two_lines.atoms.len(), 3);
    assert_eq!(two_lines.ignored_text.map(|s| (s.offset, s.length)), Some((4, 2)));
    for quiet in ["CCO ", "CCO\n", "CCO \t\r\n"] {
        let parsed = read(quiet);
        assert_eq!(parsed.ignored_text, None, "{quiet:?}");
        assert!(parsed.losses.is_empty(), "{quiet:?}");
    }
}

#[test]
fn an_unspecified_bond_between_aromatic_atoms_is_aromatic_only_on_a_ring() {
    let biphenyl = read("c1ccccc1c1ccccc1");
    let link = biphenyl
        .bonds
        .iter()
        .find(|bond| (bond.begin, bond.end) == (5, 6))
        .expect("the bond between the rings");
    assert_eq!(link.order, BondOrder::Single);
    let aromatic = biphenyl
        .bonds
        .iter()
        .filter(|bond| bond.order == BondOrder::Aromatic)
        .count();
    assert_eq!(aromatic, 12);
}

#[test]
fn aromatic_bonds_and_atoms_outside_a_ring_are_refused() {
    for (text, code, offset) in [
        ("c:c", SmilesCode::AromaticBondOutsideRing, 1),
        ("c1ccccc1:c1ccccc1", SmilesCode::AromaticBondOutsideRing, 8),
        ("c", SmilesCode::AromaticAtomOutsideRing, 0),
        ("cc", SmilesCode::AromaticAtomOutsideRing, 0),
        ("Cc", SmilesCode::AromaticAtomOutsideRing, 1),
    ] {
        let (found, found_offset, _) = refused(text);
        assert_eq!((found, found_offset), (code, offset), "{text}");
    }
}

#[test]
fn an_unsupported_aromatic_system_is_located_inside_that_system() {
    // The second component's five-carbon ring is the one the model cannot class aromatic.
    let (code, offset, _) = refused("c1ccccc1.c1cccc1");
    assert_eq!(code, SmilesCode::AromaticSystemUnsupported);
    assert!((9..16).contains(&offset), "offset {offset} is not in the second component");
    let error = OpenSmilesCodec.read("c1cccc1").unwrap_err();
    assert!(error.reason.is_some(), "the model's reason is reported");
}

#[test]
fn kekule_and_aromatic_spellings_both_read() {
    for text in [
        "c1ccccc1",
        "C1=CC=CC=C1",
        "c1cc[nH]c1",
        "C1=CNC=C1",
        "c1ccncc1",
        "c1ccc2ccccc2c1",
        "[nH]1cccc1",
        "c1ccoc1",
        "c1ccsc1",
        "[O-]c1ccccc1",
        "c1cc[n+]([O-])cc1",
    ] {
        read(text);
    }
}

#[test]
fn limits_are_reported_as_limits() {
    let long = "C".repeat(MAX_INPUT_BYTES + 1);
    assert_eq!(refused(&long), (SmilesCode::InputTooLong, MAX_INPUT_BYTES, 1));
    assert_eq!(SmilesCode::InputTooLong.class().as_str(), "limit");
    // A two-byte character that straddles the limit is reported from its first byte.
    let straddling = format!("{}é", "C".repeat(MAX_INPUT_BYTES - 1));
    assert_eq!(straddling.len(), MAX_INPUT_BYTES + 1);
    assert_eq!(
        refused(&straddling),
        (SmilesCode::InputTooLong, MAX_INPUT_BYTES - 1, 2)
    );
    // A character that ends exactly at the limit is within it. The filler follows a
    // terminator, so it is not read as atoms.
    let exact = format!("C {}é", "x".repeat(MAX_INPUT_BYTES - 4));
    assert_eq!(exact.len(), MAX_INPUT_BYTES);
    assert_eq!(read(&exact).ignored_text.map(|s| s.length), Some(MAX_INPUT_BYTES - 2));
    // Only the first four bytes of a four-byte character straddle.
    let emoji = format!("{}\u{1F600}", "C".repeat(MAX_INPUT_BYTES - 2));
    assert_eq!(
        refused(&emoji),
        (SmilesCode::InputTooLong, MAX_INPUT_BYTES - 2, 4)
    );
}

#[test]
fn one_import_holds_at_most_the_commands_of_a_transaction() {
    // 250 atoms and 249 bonds, then a ring closure makes 500: the most one transaction takes.
    let at_limit = format!("C1{}1", "C".repeat(249));
    let parsed = read(&at_limit);
    assert_eq!(parsed.atoms.len() + parsed.bonds.len(), 500);
    let over = "C".repeat(251);
    let (code, _, _) = refused(&over);
    assert_eq!(code, SmilesCode::CommandLimit);
    assert_eq!(code.class().as_str(), "limit");
}

#[test]
fn branch_depth_has_a_limit() {
    let nested = |depth: usize| format!("C{}C{}", "(C".repeat(depth), ")".repeat(depth));
    assert!(OpenSmilesCodec.read(&nested(MAX_BRANCH_DEPTH)).is_ok());
    let (code, _, _) = refused(&nested(MAX_BRANCH_DEPTH + 1));
    assert_eq!(code, SmilesCode::BranchDepthLimit);
}

#[test]
fn every_code_has_one_message_class_and_name() {
    let mut names = std::collections::BTreeSet::new();
    for code in SmilesCode::ALL {
        assert!(names.insert(code.as_str()), "{} is listed twice", code.as_str());
        assert!(!code.message().is_empty(), "{}", code.as_str());
        assert!(
            code.as_str().bytes().all(|b| b.is_ascii_lowercase() || b == b'_'),
            "{}",
            code.as_str()
        );
        assert!(
            code.message().ends_with('.'),
            "{} message should be a sentence",
            code.as_str()
        );
    }
    assert_eq!(names.len(), SmilesCode::ALL.len());
}
