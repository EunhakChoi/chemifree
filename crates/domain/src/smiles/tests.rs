//! Reader, writer and canonical-form tests.
//!
//! Expected values are written out by hand from the grammar. The property tests are seeded and
//! deterministic.

use std::collections::BTreeMap;

use super::*;
use crate::aromaticity;
use crate::chemistry;
use crate::document::{
    Atom, AtomId, Bond, BondId, BondOrder, MAX_CHARGE, MAX_COMMANDS, MIN_CHARGE,
};
use crate::element::Element;
use crate::graph::MoleculeGraph;
use crate::identity;
use crate::states::{AtomKind, Hydrogens, Radical};

// ---------------------------------------------------------------------------------------------
// Helpers

fn fail(text: &str) -> (ErrorCode, (usize, usize)) {
    let error = read(text).expect_err(text);
    let span = error.span.expect("a read error has a span");
    (error.code, (span.start, span.end))
}

fn graph_of(text: &str) -> MoleculeGraph {
    read(text)
        .unwrap_or_else(|e| panic!("{text}: {e:?}"))
        .graph()
}

fn canonical(text: &str) -> String {
    write_canonical(&graph_of(text))
        .unwrap_or_else(|e| panic!("{text}: {e:?}"))
        .text
}

/// A deterministic xorshift generator, so failures reproduce from the seed alone.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(n).unwrap()).unwrap()
    }
    fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            items.swap(i, self.below(i + 1));
        }
    }
}

fn atom_id(n: usize) -> AtomId {
    AtomId::from_serial(u32::try_from(n).unwrap()).unwrap()
}
fn bond_id(n: usize) -> BondId {
    BondId::from_serial(u32::try_from(n).unwrap()).unwrap()
}

fn element_atom(symbol: &str) -> Atom {
    Atom::new(AtomKind::Element(Element::from_symbol(symbol).unwrap()))
}

fn build(atoms: Vec<Atom>, bonds: Vec<(usize, usize, BondOrder)>) -> MoleculeGraph {
    MoleculeGraph::from_parts(
        atoms
            .into_iter()
            .enumerate()
            .map(|(i, a)| (atom_id(i + 1), a))
            .collect(),
        bonds
            .into_iter()
            .enumerate()
            .map(|(i, (a, b, order))| {
                (
                    bond_id(i + 1),
                    Bond {
                        begin: atom_id(a + 1),
                        end: atom_id(b + 1),
                        order,
                    },
                )
            })
            .collect(),
    )
}

/// The same graph with its atoms, bonds, IDs and bond directions shuffled.
fn permuted(graph: &MoleculeGraph, rng: &mut Rng) -> MoleculeGraph {
    let atoms: Vec<(AtomId, Atom)> = graph.atoms().map(|(id, a)| (id, a.clone())).collect();
    let mut new_serial: Vec<usize> = (1..=atoms.len()).collect();
    rng.shuffle(&mut new_serial);
    let rename: BTreeMap<AtomId, AtomId> = atoms
        .iter()
        .zip(&new_serial)
        .map(|((id, _), &n)| (*id, atom_id(n)))
        .collect();
    let mut atoms: Vec<(AtomId, Atom)> =
        atoms.into_iter().map(|(id, a)| (rename[&id], a)).collect();
    rng.shuffle(&mut atoms);
    let mut bonds: Vec<(BondId, Bond)> = graph.bonds().map(|(id, b)| (id, b.clone())).collect();
    for (_, bond) in &mut bonds {
        bond.begin = rename[&bond.begin];
        bond.end = rename[&bond.end];
        if rng.below(2) == 0 && bond.order != BondOrder::Dative {
            std::mem::swap(&mut bond.begin, &mut bond.end);
        }
    }
    rng.shuffle(&mut bonds);
    MoleculeGraph::from_parts(atoms, bonds)
}

/// Rewrites aromatic bonds as the model's Kekulé assignment, and gives back the nitrogen
/// hydrogens that assignment makes redundant: the Kekulé spelling of the same molecule.
fn kekule_spelling(graph: &MoleculeGraph) -> MoleculeGraph {
    let assignment = aromaticity::kekulize(graph).expect("the model supports this system");
    let redundant = aromaticity::redundant_nitrogen_hydrogens(graph, &assignment);
    let orders: BTreeMap<BondId, BondOrder> = assignment
        .into_iter()
        .map(|(id, order)| (id, order.bond_order()))
        .collect();
    MoleculeGraph::from_parts(
        graph
            .atoms()
            .map(|(id, atom)| {
                let mut atom = atom.clone();
                if redundant.contains(&id) {
                    atom.hydrogens = Hydrogens::Inferred;
                }
                (id, atom)
            })
            .collect(),
        graph
            .bonds()
            .map(|(id, bond)| {
                let mut bond = bond.clone();
                if let Some(order) = orders.get(&id) {
                    bond.order = order.clone();
                }
                (id, bond)
            })
            .collect(),
    )
}

/// The chemical meaning of a written structure in the order the writer named its atoms:
/// element, charge, isotope, radical, resolved hydrogens and the bonds between positions.
type Meaning = (Vec<String>, Vec<(usize, usize, String)>);

fn meaning(graph: &MoleculeGraph, order: &[AtomId]) -> Meaning {
    let position: BTreeMap<AtomId, usize> =
        order.iter().enumerate().map(|(i, &id)| (id, i)).collect();
    let kekule = if graph.bonds().any(|(_, b)| b.order == BondOrder::Aromatic) {
        aromaticity::kekulize(graph)
            .ok()
            .map(|v| v.into_iter().collect::<BTreeMap<_, _>>())
    } else {
        Some(BTreeMap::new())
    };
    let assignment = kekule.unwrap_or_default();
    let orders = chemistry::incident_orders(
        graph.atoms().map(|(id, _)| id),
        graph.bonds(),
        |id, bond| aromaticity::kekule_order_value(&assignment, id, bond),
    );
    let mut atoms = vec![String::new(); order.len()];
    for (id, atom) in graph.atoms() {
        let Some(&at) = position.get(&id) else {
            continue;
        };
        let assessed = chemistry::assess(id, atom, &orders[&id]);
        let hydrogens = match atom.hydrogens {
            Hydrogens::Specified(n) => u16::from(n),
            _ => assessed.hydrogen_count.unwrap_or(0),
        };
        atoms[at] = format!(
            "{} {} {:?} {:?} H{}",
            atom.kind.element().unwrap().symbol(),
            atom.charge,
            atom.isotope,
            atom.radical,
            hydrogens
        );
    }
    let mut bonds: Vec<(usize, usize, String)> = graph
        .bonds()
        .filter_map(|(_, bond)| {
            let (a, b) = (position.get(&bond.begin)?, position.get(&bond.end)?);
            Some((*a.min(b), *a.max(b), format!("{:?}", bond.order)))
        })
        .collect();
    bonds.sort();
    (atoms, bonds)
}

// ---------------------------------------------------------------------------------------------
// Reader: accepted grammar

#[test]
fn reads_chains_branches_and_rings() {
    let cases: [(&str, usize, usize, usize); 14] = [
        // (input, atoms, bonds, components)
        ("CCO", 3, 2, 1),
        ("C(C)(C)C", 4, 3, 1),
        ("C1CCCCC1", 6, 6, 1),
        ("C%10CCCCC%10", 6, 6, 1),
        ("C%99CC%99", 3, 3, 1),
        // One atom closing a ring and opening another with the same label: a spiro centre.
        ("C1CCC11CCCC1", 8, 9, 1),
        ("C.C", 2, 0, 2),
        ("C1.C1", 2, 1, 1),
        ("[Na+].[Cl-]", 2, 0, 2),
        ("C(.C)C", 3, 1, 2),
        ("ClCBr", 3, 2, 1),
        ("[H][H]", 2, 1, 1),
        ("C=1CCCCC=1", 6, 6, 1),
        ("C0CC0", 3, 3, 1),
    ];
    for (text, atoms, bonds, components) in cases {
        let read = read(text).unwrap_or_else(|e| panic!("{text}: {e:?}"));
        assert_eq!(
            (read.atoms.len(), read.bonds.len(), read.components),
            (atoms, bonds, components),
            "{text}"
        );
    }
}

#[test]
fn a_ring_bond_symbol_may_sit_on_either_end() {
    for text in ["C=1CCCCC=1", "C=1CCCCC1", "C1CCCCC=1"] {
        let read = read(text).unwrap();
        assert_eq!(
            read.bonds
                .iter()
                .filter(|b| b.order == BondOrder::Double)
                .count(),
            1,
            "{text}"
        );
    }
}

#[test]
fn bracket_atoms_carry_isotope_hydrogens_and_charge() {
    let atom = |text: &str| read(text).unwrap().atoms[0].atom.clone();
    let carbon = atom("[13CH2+]");
    assert_eq!(
        (carbon.isotope, carbon.hydrogens, carbon.charge),
        (Some(13), Hydrogens::Specified(2), 1)
    );
    // OpenSMILES allows leading zeros in the isotope.
    assert_eq!(atom("[002H]").isotope, Some(2));
    assert_eq!(atom("[999U]").isotope, Some(999));
    assert_eq!(atom("[HH]").hydrogens, Hydrogens::Specified(1));
    assert_eq!(atom("[O--]").charge, -2);
    assert_eq!(atom("[Fe+3]").charge, 3);
    assert_eq!(atom("[N+0]").charge, 0);
    assert_eq!(atom("[N-0]").charge, 0);
    assert_eq!(atom("[C]").hydrogens, Hydrogens::Specified(0));
    assert_eq!(atom("C").hydrogens, Hydrogens::Inferred);
    assert_eq!(
        atom(&format!("[Fe{MAX_CHARGE:+}]")).charge,
        MAX_CHARGE,
        "the largest charge the drawing records"
    );
    assert_eq!(atom(&format!("[Fe{MIN_CHARGE}]")).charge, MIN_CHARGE);
}

#[test]
fn an_unwritten_bond_is_aromatic_only_as_a_ring_bond() {
    let biphenyl = read("c1ccccc1c1ccccc1").unwrap();
    let aromatic = biphenyl
        .bonds
        .iter()
        .filter(|b| b.order == BondOrder::Aromatic)
        .count();
    assert_eq!(aromatic, 12);
    let bridge = &biphenyl
        .bonds
        .iter()
        .find(|b| b.begin == 5 && b.end == 6)
        .unwrap()
        .order;
    assert_eq!(
        *bridge,
        BondOrder::Single,
        "the bond between the rings is a bridge"
    );
    // The same text with an explicit single bond reads the same.
    assert_eq!(read("c1ccccc1-c1ccccc1").unwrap().bonds, biphenyl.bonds);
}

#[test]
fn whitespace_ends_the_smiles_and_trailing_text_is_reported() {
    let cases: [(&str, Option<(usize, usize)>); 6] = [
        ("CCO", None),
        ("CCO ", None),
        ("CCO  \n", None),
        ("CCO ethanol", Some((4, 11))),
        ("CCO\tname with spaces\r\n", Some((4, 20))),
        ("C C", Some((2, 3))),
    ];
    for (text, ignored) in cases {
        let read = read(text).unwrap();
        assert_eq!(
            read.ignored_text.map(|s| (s.start, s.end)),
            ignored,
            "{text:?}"
        );
        assert_eq!(
            read.atoms.len(),
            if text.starts_with("CCO") { 3 } else { 1 }
        );
    }
    // Text that merely looks like more SMILES is still not read.
    assert_eq!(read("C CC").unwrap().atoms.len(), 1);
}

// ---------------------------------------------------------------------------------------------
// Reader: errors, codes and spans

#[test]
fn errors_carry_the_code_and_span_of_the_offending_text() {
    use ErrorCode as E;
    let cases: &[(&str, E, (usize, usize))] = &[
        // Framing.
        ("", E::EmptyInput, (0, 0)),
        (" C", E::LeadingWhitespace, (0, 1)),
        ("\t\nC", E::LeadingWhitespace, (0, 2)),
        ("   ", E::LeadingWhitespace, (0, 3)),
        // Bond symbols.
        ("C=(O)", E::DanglingBond, (1, 2)),
        ("C=)", E::DanglingBond, (1, 2)),
        ("C(=)C", E::DanglingBond, (2, 3)),
        ("C=.C", E::DanglingBond, (1, 2)),
        ("C=", E::DanglingBond, (1, 2)),
        ("C= x", E::DanglingBond, (1, 2)),
        ("C1=", E::DanglingBond, (2, 3)),
        ("=C", E::BondWithoutAtom, (0, 1)),
        ("C.=C", E::BondAfterDot, (2, 3)),
        ("C==C", E::RepeatedBond, (2, 3)),
        // Dots and branches.
        (".C", E::DotWithoutAtom, (0, 1)),
        ("C.", E::DotWithoutAtom, (1, 2)),
        ("C..C", E::DotWithoutAtom, (2, 3)),
        ("C.)", E::DotWithoutAtom, (1, 2)),
        ("(C)", E::BranchWithoutAtom, (0, 1)),
        ("C((C))", E::BranchWithoutAtom, (2, 3)),
        ("C.(C)", E::DotWithoutAtom, (1, 2)),
        ("C()", E::EmptyBranch, (2, 3)),
        ("C)", E::UnmatchedCloseBranch, (1, 2)),
        ("C(C", E::UnclosedBranch, (1, 2)),
        ("C(C(C", E::UnclosedBranch, (1, 2)),
        // Ring digits sit directly after their atom (or its bond symbol).
        ("1C", E::RingDigitWithoutAtom, (0, 1)),
        ("C.1", E::RingDigitWithoutAtom, (2, 3)),
        ("C(1)", E::RingDigitAfterBranchOpen, (2, 3)),
        ("C(=1)", E::RingDigitAfterBranchOpen, (3, 4)),
        ("C(C)1", E::RingDigitAfterBranchClose, (4, 5)),
        ("C(C)=1", E::RingDigitAfterBranchClose, (5, 6)),
        ("C%1C", E::RingLabelSyntax, (1, 3)),
        ("C%", E::RingLabelSyntax, (1, 2)),
        ("C%(12)", E::ExtendedRingLabelUnsupported, (1, 6)),
        ("C%(123456)C", E::ExtendedRingLabelUnsupported, (1, 10)),
        ("C%(", E::RingLabelSyntax, (1, 2)),
        ("C%(1", E::RingLabelSyntax, (1, 2)),
        ("C%()", E::RingLabelSyntax, (1, 2)),
        ("C%(x)", E::RingLabelSyntax, (1, 2)),
        ("C11", E::RingClosureSelf, (2, 3)),
        ("C1C1", E::RingClosureDuplicateBond, (3, 4)),
        ("C12C12", E::RingClosureDuplicateBond, (4, 5)),
        ("C12CC12", E::RingClosureDuplicateBond, (6, 7)),
        ("C=1CC-1", E::RingBondConflict, (6, 7)),
        // The earliest-opened unclosed ring, by position and not by label.
        ("C1CC", E::UnclosedRing, (1, 2)),
        ("C2CC1CC", E::UnclosedRing, (1, 2)),
        ("C1CC2CC", E::UnclosedRing, (1, 2)),
        ("C=2CC1CC", E::UnclosedRing, (1, 3)),
        // Atoms.
        ("X", E::BareAtomNotOrganic, (0, 1)),
        ("Na", E::BareAtomNotOrganic, (1, 2)),
        ("[C", E::UnclosedBracketAtom, (0, 1)),
        ("[C H]", E::UnclosedBracketAtom, (0, 1)),
        ("[]", E::MissingElement, (1, 2)),
        ("[13]", E::MissingElement, (3, 4)),
        ("[+]", E::MissingElement, (1, 2)),
        ("[Xx]", E::UnknownElement, (1, 3)),
        ("[q]", E::UnknownElement, (1, 2)),
        ("[CH2x]", E::BracketUnexpectedCharacter, (4, 5)),
        ("[1234C]", E::IsotopeTooLong, (1, 5)),
        ("[C+9]", E::ChargeBeyondDrawingUnsupported, (2, 4)),
        ("[C-9]", E::ChargeBeyondDrawingUnsupported, (2, 4)),
        ("[C+15]", E::ChargeBeyondDrawingUnsupported, (2, 5)),
        ("[C-15]", E::ChargeBeyondDrawingUnsupported, (2, 5)),
        ("[C+16]", E::ChargeOutOfRange, (2, 5)),
        ("[C+++++++++]", E::ChargeBeyondDrawingUnsupported, (2, 11)),
        ("[C+99]", E::ChargeOutOfRange, (2, 5)),
        ("[C+123]", E::ChargeSyntax, (2, 6)),
        ("[C+-]", E::ChargeSyntax, (2, 4)),
        ("[C++1]", E::ChargeSyntax, (2, 5)),
        // Unsupported features carry their own codes.
        ("[0C]", E::IsotopeZeroUnsupported, (1, 2)),
        ("[000C]", E::IsotopeZeroUnsupported, (1, 4)),
        ("[C@H](F)(Cl)Br", E::ChiralityUnsupported, (2, 3)),
        ("[C@@H]", E::ChiralityUnsupported, (2, 3)),
        ("C/C=C/C", E::DirectionalBondUnsupported, (1, 2)),
        ("C\\C", E::DirectionalBondUnsupported, (1, 2)),
        ("C*", E::WildcardUnsupported, (1, 2)),
        ("[*]", E::WildcardUnsupported, (1, 2)),
        ("C$C", E::QuadrupleBondUnsupported, (1, 2)),
        ("[CH4:1]", E::AtomClassUnsupported, (4, 5)),
        ("CC>>CO", E::ReactionUnsupported, (2, 3)),
        // Non-ASCII text: the span is the whole character.
        ("Cé", E::UnexpectedCharacter, (1, 3)),
        ("C€", E::UnexpectedCharacter, (1, 4)),
        ("[é]", E::UnexpectedCharacter, (1, 3)),
        ("[C€]", E::UnexpectedCharacter, (2, 5)),
        ("C😀C", E::UnexpectedCharacter, (1, 5)),
        ("C\u{0}", E::UnexpectedCharacter, (1, 2)),
        // Aromaticity.
        ("c", E::AromaticAtomOutsideRing, (0, 1)),
        ("c:c", E::AromaticBondOutsideRing, (1, 2)),
        ("c1ccccc1:c1ccccc1", E::AromaticBondOutsideRing, (8, 9)),
        ("C1cC1", E::AromaticAtomNotInAromaticRing, (2, 3)),
    ];
    let wrong: Vec<String> = cases
        .iter()
        .filter_map(|&(text, code, span)| {
            let got = fail(text);
            (got != (code, span))
                .then(|| format!("{text:?}: expected {:?}, got {got:?}", (code, span)))
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn unsupported_aromatic_systems_name_the_models_reason() {
    for (text, reason) in [
        ("c1cccc1", "valence_not_in_model"),
        ("c1ccc2[nH]ccc2c1", "fused_system_outside_model"),
    ] {
        let error = read(text).expect_err(text);
        assert_eq!(error.code, ErrorCode::AromaticSystemUnsupported, "{text}");
        assert_eq!(error.class(), ErrorClass::Unsupported);
        assert!(error.reason.is_some(), "{text}: no reason");
        let _ = reason;
    }
}

#[test]
fn a_bad_ring_bond_never_leaves_a_partial_read() {
    // The error is the first problem in reading order, so a later one is not reported.
    assert_eq!(
        fail("C(=1)C%"),
        (ErrorCode::RingDigitAfterBranchOpen, (3, 4))
    );
}

// ---------------------------------------------------------------------------------------------
// Reader: limits

#[test]
fn the_command_limit_counts_atoms_and_bonds_and_is_a_limit() {
    // n atoms in a chain are n + n - 1 commands.
    let fits = "C".repeat(MAX_COMMANDS.div_ceil(2));
    assert!(read(&fits).is_ok());
    let over = "C".repeat(MAX_COMMANDS.div_ceil(2) + 1);
    let error = read(&over).unwrap_err();
    assert_eq!(error.code, ErrorCode::CommandLimit);
    assert_eq!(error.class(), ErrorClass::Limit);
    let last = MAX_COMMANDS.div_ceil(2);
    assert_eq!(error.span, Some(Span::new(last, last + 1)));
    // Isolated atoms cost one command each.
    let dots = vec!["C"; MAX_COMMANDS].join(".");
    assert!(dots.len() <= MAX_INPUT_BYTES);
    assert!(read(&dots).is_ok());
    let error = read(&format!("{dots}.C")).unwrap_err();
    assert_eq!(error.code, ErrorCode::CommandLimit);
    // A closing ring bond is a command too.
    let ring = format!("C1{}1", "C".repeat(MAX_COMMANDS / 2 - 1));
    assert_eq!(read(&ring).unwrap().bonds.len(), MAX_COMMANDS / 2);
    let error = read(&format!("C1{}1", "C".repeat(MAX_COMMANDS / 2))).unwrap_err();
    assert_eq!(error.code, ErrorCode::CommandLimit);
}

#[test]
fn branches_nest_to_the_depth_limit_and_no_further() {
    let nested = |depth: usize| format!("C{}{}", "(C".repeat(depth), ")".repeat(depth));
    assert!(read(&nested(MAX_BRANCH_DEPTH)).is_ok());
    let error = read(&nested(MAX_BRANCH_DEPTH + 1)).unwrap_err();
    assert_eq!(error.code, ErrorCode::BranchDepthLimit);
    assert_eq!(error.class(), ErrorClass::Limit);
    let at = 1 + 2 * MAX_BRANCH_DEPTH;
    assert_eq!(error.span, Some(Span::new(at, at + 1)));
}

#[test]
fn input_longer_than_the_limit_is_refused_at_a_character_boundary() {
    let long = "C".repeat(MAX_INPUT_BYTES + 1);
    let error = read(&long).unwrap_err();
    assert_eq!(error.code, ErrorCode::InputTooLong);
    assert_eq!(error.class(), ErrorClass::Limit);
    assert_eq!(
        error.span,
        Some(Span::new(MAX_INPUT_BYTES, MAX_INPUT_BYTES + 1))
    );
    // A two-byte character straddling the limit: the span starts before it, on a boundary.
    let straddling = format!("{}é", "C".repeat(MAX_INPUT_BYTES - 1));
    assert_eq!(straddling.len(), MAX_INPUT_BYTES + 1);
    let error = read(&straddling).unwrap_err();
    assert_eq!(error.code, ErrorCode::InputTooLong);
    let span = error.span.unwrap();
    assert_eq!(
        (span.start, span.end),
        (MAX_INPUT_BYTES - 1, MAX_INPUT_BYTES + 1)
    );
    assert!(straddling.is_char_boundary(span.start) && straddling.is_char_boundary(span.end));
    // Three- and four-byte characters at every offset around the limit.
    for filler in ["€", "😀"] {
        for pad in 0..4 {
            let text = format!("{}{filler}{filler}", "C".repeat(MAX_INPUT_BYTES - pad));
            let span = read(&text).unwrap_err().span.unwrap();
            assert!(
                text.is_char_boundary(span.start) && text.is_char_boundary(span.end),
                "{filler} pad {pad}: {span:?}"
            );
        }
    }
    // Exactly at the limit is accepted, trailing text and all.
    let at_limit = format!("C {}", "x".repeat(MAX_INPUT_BYTES - 2));
    assert_eq!(at_limit.len(), MAX_INPUT_BYTES);
    assert_eq!(
        read(&at_limit).unwrap().ignored_text,
        Some(Span::new(2, MAX_INPUT_BYTES))
    );
}

// ---------------------------------------------------------------------------------------------
// Reader: radical and hydrogen state from the shared valence model

#[test]
fn a_bracket_atoms_radical_state_comes_from_the_valence_model() {
    use RadicalNote::{Inferred, Stated, Undetermined};
    let cases: [(&str, usize, Radical, RadicalNote); 10] = [
        ("[CH4]", 0, Radical::None, Stated),
        ("[CH3]", 0, Radical::Doublet, Inferred),
        ("C[CH2]", 1, Radical::Doublet, Inferred),
        ("[OH-]", 0, Radical::None, Stated),
        ("[NH4+]", 0, Radical::None, Stated),
        ("[H]", 0, Radical::Doublet, Inferred),
        // Two open valences are a singlet or a triplet; the model cannot say which.
        ("[CH2]", 0, Radical::Unknown, Undetermined),
        // Outside the valence model's table: honestly undetermined.
        ("[Na+]", 0, Radical::Unknown, Undetermined),
        // No single valence-consistent state: unknown, and reported, never silently none.
        ("[CH]", 0, Radical::Unknown, Undetermined),
        ("[C]", 0, Radical::Unknown, Undetermined),
    ];
    for (text, index, radical, note) in cases {
        let read = read(text).unwrap();
        assert_eq!(read.atoms[index].atom.radical, radical, "{text}");
        assert_eq!(read.atoms[index].radical_note, note, "{text}");
    }
    // A bare organic atom never gets a radical: its hydrogens are inferred.
    assert_eq!(read("C").unwrap().atoms[0].atom.radical, Radical::None);
}

#[test]
fn aromatic_bracket_atoms_use_the_kekule_assignment_for_their_state() {
    let read = read("c1cc[nH]c1").unwrap();
    assert!(read.atoms.iter().all(|a| a.atom.radical == Radical::None));
    assert_eq!(read.atoms[3].atom.hydrogens, Hydrogens::Specified(1));
    let read = super::read("c1cc[cH-]c1").unwrap();
    assert_eq!(read.atoms[3].atom.radical, Radical::None);
}

// ---------------------------------------------------------------------------------------------
// Error table

#[test]
fn every_error_code_has_a_unique_code_and_message_both_ways() {
    let mut codes = std::collections::BTreeSet::new();
    let mut messages = std::collections::BTreeSet::new();
    for &code in ErrorCode::ALL {
        assert!(!code.message().is_empty(), "{code:?} has no message");
        assert!(
            code.message().ends_with('.'),
            "{code:?}: {}",
            code.message()
        );
        assert!(codes.insert(code.code()), "{code:?}: duplicate code");
        assert!(
            messages.insert(code.message()),
            "{code:?}: duplicate message"
        );
        assert_eq!(ErrorCode::from_code(code.code()), Some(code));
        assert!(
            code.code()
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b == b'_'),
            "{}",
            code.code()
        );
    }
    assert_eq!(ErrorCode::from_code("no_such_code"), None);
    assert_eq!(codes.len(), ErrorCode::ALL.len());
}

#[test]
fn every_error_code_is_documented_in_the_contract() {
    let contract = include_str!("../../../../docs/CONTRACT.md");
    for &code in ErrorCode::ALL {
        assert!(
            contract.contains(&format!("`{}`", code.code())),
            "docs/CONTRACT.md does not document `{}`",
            code.code()
        );
    }
}

// ---------------------------------------------------------------------------------------------
// Reader: no panics

fn assert_total(text: &str) {
    if let Ok(read) = read(text) {
        // An accepted structure can always be handed on.
        let graph = read.graph();
        let _ = write_recorded(&graph);
        let _ = write_canonical(&graph);
    } else if let Err(error) = read(text)
        && let Some(span) = error.span
    {
        assert!(
            span.start <= span.end
                && span.end <= text.len()
                && text.is_char_boundary(span.start)
                && text.is_char_boundary(span.end),
            "{text:?}: bad span {span:?} for {:?}",
            error.code
        );
    }
}

#[test]
fn awkward_inputs_never_panic() {
    let huge = "(".repeat(100_000);
    let digits = "1".repeat(50_000);
    let percent = "%".repeat(10_000);
    let brackets = "[".repeat(10_000);
    let mut inputs: Vec<&str> = vec![
        "",
        " ",
        "\n",
        "\0",
        "[",
        "]",
        "%",
        "%%",
        "%1",
        "%12",
        "(",
        ")",
        "()",
        "((",
        "C(",
        "C)",
        "=",
        "C=",
        "[C",
        "[C@",
        "[CH",
        "[CH2+",
        "[+",
        "[-",
        "1",
        "C1",
        "C%1",
        "é",
        "[é",
        "😀",
        "\u{feff}C",
        "C\u{a0}C",
        "[H+]",
        "[H]",
        "[2H]",
        "[HH]",
        "c1",
        "c1c",
        "n",
        "[nH]",
        "[nH]1cccc1",
        "C1CC1C1CC1",
        "C12C1",
        "[13C@H]",
        "C#N",
        "C#",
        "$",
        ":",
        ".",
        "..",
        "C.C.C",
        "%99%99",
    ];
    inputs.extend([
        huge.as_str(),
        digits.as_str(),
        percent.as_str(),
        brackets.as_str(),
    ]);
    for text in inputs {
        assert_total(text);
    }
}

#[test]
fn seeded_fuzzing_never_panics_and_spans_stay_on_boundaries() {
    let alphabet: Vec<&str> = vec![
        "C", "N", "O", "c", "n", "o", "s", "S", "Cl", "Br", "F", "I", "P", "B", "[", "]", "(", ")",
        "=", "#", ":", ".", "-", "+", "%", "0", "1", "2", "3", "9", "H", "@", "/", "\\", "*", " ",
        "\t", "\n", "é", "€", "😀", "Na", "13", "00", "--", "++", "[nH]", "[O-]", "[NH4+]",
        "c1ccccc1", "C1", "C%10", "[2H]",
    ];
    let seeds = [
        "CCO",
        "c1ccccc1",
        "C1CC1",
        "CC(=O)O",
        "[NH4+].[Cl-]",
        "C1=CC=CC=C1",
        "C%10CC%10",
    ];
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for round in 0..40_000 {
        let mut text = String::new();
        if round % 3 == 0 {
            // Mutate a valid string.
            text.push_str(seeds[rng.below(seeds.len())]);
            for _ in 0..1 + rng.below(4) {
                let at = {
                    let mut at = rng.below(text.len() + 1);
                    while !text.is_char_boundary(at) {
                        at -= 1;
                    }
                    at
                };
                match rng.below(3) {
                    0 => text.insert_str(at, alphabet[rng.below(alphabet.len())]),
                    1 => {
                        if at < text.len() {
                            let end = at + text[at..].chars().next().unwrap().len_utf8();
                            text.replace_range(at..end, "");
                        }
                    }
                    _ => {
                        if at < text.len() {
                            let end = at + text[at..].chars().next().unwrap().len_utf8();
                            text.replace_range(at..end, alphabet[rng.below(alphabet.len())]);
                        }
                    }
                }
            }
        } else {
            for _ in 0..rng.below(24) {
                text.push_str(alphabet[rng.below(alphabet.len())]);
            }
        }
        assert_total(&text);
    }
}

// ---------------------------------------------------------------------------------------------
// Recorded writer

fn spelled(text: &str) -> String {
    write_recorded(&graph_of(text)).unwrap().text
}

#[test]
fn the_recorded_writer_keeps_the_recorded_order_and_states() {
    for text in [
        "CCO",
        "OCC",
        "C(C)(C)C",
        "C1CCCCC1",
        "C1=CC=CC=C1",
        "c1ccccc1",
        "c1cc[nH]c1",
        "[NH4+]",
        "[13CH3]O",
        "[Na+].[Cl-]",
        "[H]O[H]",
        "C#N",
        "C=1CCCCC1",
    ] {
        let graph = graph_of(text);
        let written = write_recorded(&graph).unwrap();
        let ids: Vec<AtomId> = graph.atoms().map(|(id, _)| id).collect();
        assert_eq!(written.order, ids, "{text}: recorded order of atoms");
        let again = graph_of(&written.text);
        assert_eq!(
            meaning(&graph, &ids),
            meaning(&again, &again.atoms().map(|(id, _)| id).collect::<Vec<_>>()),
            "{text} -> {}",
            written.text
        );
    }
    assert_eq!(spelled("OCC"), "OCC");
    assert_eq!(spelled("C(C)(C)C"), "C(C)(C)C");
}

#[test]
fn inferred_hydrogens_of_bracket_atoms_are_written_from_the_valence_model() {
    // A drawn N+ with four bonds-worth of hydrogens: inferred in the drawing, stated in the text.
    let mut ammonium = element_atom("N");
    ammonium.charge = 1;
    let graph = build(vec![ammonium], vec![]);
    let written = write_recorded(&graph).unwrap();
    assert_eq!(written.text, "[NH4+]");
    assert_eq!(written.hydrogen_state_changes, vec![atom_id(1)]);
    // Isotope and radical atoms too.
    let mut deuterio = element_atom("C");
    deuterio.isotope = Some(13);
    let methyl = build(
        vec![deuterio, element_atom("O")],
        vec![(0, 1, BondOrder::Single)],
    );
    assert_eq!(write_recorded(&methyl).unwrap().text, "[13CH3]O");
    let mut radical = element_atom("C");
    radical.radical = Radical::Doublet;
    assert_eq!(
        write_recorded(&build(vec![radical], vec![])).unwrap().text,
        "[CH3]"
    );
    // A stated count is written as stated, even where inference would differ.
    let mut stated = element_atom("C");
    stated.hydrogens = Hydrogens::Specified(3);
    let graph = build(
        vec![stated, element_atom("C")],
        vec![(0, 1, BondOrder::Single)],
    );
    let written = write_recorded(&graph).unwrap();
    assert_eq!(written.text, "[CH3]C");
    assert!(
        written.hydrogen_state_changes.is_empty(),
        "a stated count stays stated"
    );
}

#[test]
fn the_writer_refuses_what_the_reader_would_not_accept_and_says_why() {
    use ErrorCode as E;
    let refused = |graph: MoleculeGraph| write_recorded(&graph).unwrap_err();
    let carbon = || element_atom("C");

    assert_eq!(refused(build(vec![], vec![])).code, E::EmptyGraph);

    let mut query = carbon();
    query.kind = AtomKind::Query(crate::states::QueryAtom::Any);
    let error = refused(build(vec![query], vec![]));
    assert_eq!(
        (error.code, error.subject),
        (E::QueryAtomNotWritable, Some(Subject::Atom(atom_id(1))))
    );

    for bad in [0_u16, 1000, u16::MAX] {
        let mut atom = carbon();
        atom.isotope = Some(bad);
        assert_eq!(
            refused(build(vec![atom], vec![])).code,
            E::IsotopeNotWritable,
            "isotope {bad}"
        );
    }
    for bad in [MAX_CHARGE + 1, MIN_CHARGE - 1, i8::MAX, i8::MIN] {
        let mut atom = carbon();
        atom.charge = bad;
        assert_eq!(
            refused(build(vec![atom], vec![])).code,
            E::ChargeNotWritable,
            "charge {bad}"
        );
    }
    let mut unknown = carbon();
    unknown.hydrogens = Hydrogens::Unknown;
    assert_eq!(
        refused(build(vec![unknown], vec![])).code,
        E::HydrogensUnknownNotWritable
    );

    let mut many = carbon();
    many.hydrogens = Hydrogens::Specified(10);
    assert_eq!(
        refused(build(vec![many], vec![])).code,
        E::HydrogenCountNotWritable
    );

    for order in [BondOrder::Dative, BondOrder::Unknown] {
        let error = refused(build(vec![carbon(), carbon()], vec![(0, 1, order)]));
        assert_eq!(
            (error.code, error.subject),
            (E::BondKindNotWritable, Some(Subject::Bond(bond_id(1))))
        );
    }
}

#[test]
fn ions_and_metals_outside_the_valence_table_are_written_with_their_assumptions_reported() {
    // A drawn Na+ with default (inferred) hydrogens and no radical.
    let mut sodium = element_atom("Na");
    sodium.charge = 1;
    let written = write_recorded(&build(vec![sodium.clone()], vec![])).unwrap();
    assert_eq!(written.text, "[Na+]");
    assert_eq!(written.hydrogen_assumptions, vec![atom_id(1)]);
    assert_eq!(
        written.radical_changes,
        vec![RadicalChange {
            atom: atom_id(1),
            recorded: Radical::None,
            read_back: Radical::Unknown
        }]
    );
    // Iron with default settings, neutral.
    let iron = write_recorded(&build(vec![element_atom("Fe")], vec![])).unwrap();
    assert_eq!(iron.text, "[Fe]");
    assert_eq!(iron.hydrogen_assumptions.len(), 1);
    // With hydrogens stated there is no assumption; with the radical unknown nothing changes.
    sodium.hydrogens = Hydrogens::Specified(0);
    sodium.radical = Radical::Unknown;
    let exact = write_recorded(&build(vec![sodium.clone()], vec![])).unwrap();
    assert_eq!(exact.text, "[Na+]");
    assert!(exact.hydrogen_assumptions.is_empty() && exact.radical_changes.is_empty());
    // Stated hydrogens and a recorded "none": only the radical is reported.
    sodium.radical = Radical::None;
    let stated = write_recorded(&build(vec![sodium], vec![])).unwrap();
    assert!(stated.hydrogen_assumptions.is_empty());
    assert_eq!(stated.radical_changes.len(), 1);
    // The reader accepts the text, and a salt written in recorded order reads back.
    let mut chloride = element_atom("Cl");
    chloride.charge = -1;
    let mut cation = element_atom("Na");
    cation.charge = 1;
    let salt = write_recorded(&build(vec![cation, chloride], vec![])).unwrap();
    assert_eq!(salt.text, "[Na+].[Cl-]");
    assert_eq!(salt.hydrogen_assumptions, vec![atom_id(1)]);
    // The canonical form still has no answer: the identity model needs a valence entry.
    let error = write_canonical(&graph_of("[Na+].[Cl-]")).unwrap_err();
    assert_eq!(
        (error.code, error.reason),
        (ErrorCode::CanonicalUnsupported, Some("element_or_charge"))
    );
}

#[test]
fn a_recorded_radical_the_reader_would_not_infer_is_written_and_reported() {
    // A singlet carbene reads back as an unknown radical: written, and the change named.
    let mut singlet = element_atom("C");
    singlet.radical = Radical::Singlet;
    singlet.hydrogens = Hydrogens::Specified(2);
    let written = write_recorded(&build(vec![singlet], vec![])).unwrap();
    assert_eq!(written.text, "[CH2]");
    assert_eq!(
        written.radical_changes,
        vec![RadicalChange {
            atom: atom_id(1),
            recorded: Radical::Singlet,
            read_back: Radical::Unknown
        }]
    );
    // A drawn doublet methyl needs no report.
    let mut methyl = element_atom("C");
    methyl.radical = Radical::Doublet;
    assert!(
        write_recorded(&build(vec![methyl], vec![]))
            .unwrap()
            .radical_changes
            .is_empty()
    );
}

#[test]
fn a_bracket_atom_with_no_valence_consistent_radical_is_unknown_not_none() {
    for text in ["[C]", "[CH]", "[N]", "[CH2]", "[CH5]"] {
        let read = read(text).unwrap();
        assert_eq!(read.atoms[0].atom.radical, Radical::Unknown, "{text}");
        assert_eq!(
            read.atoms[0].radical_note,
            RadicalNote::Undetermined,
            "{text}"
        );
    }
}

#[test]
fn an_aromatic_system_error_points_into_the_system_that_failed() {
    // The first ring is benzene; the second, five aromatic carbons, is not supported.
    let error = read("c1ccccc1.c1cccc1").unwrap_err();
    assert_eq!(error.code, ErrorCode::AromaticSystemUnsupported);
    let span = error.span.unwrap();
    assert!(
        span.start >= 9,
        "the span {span:?} is in the first component"
    );
    assert_eq!((span.start, span.end), (9, 10));
    // Written the other way round, the span moves with the failing system.
    let error = read("c1cccc1.c1ccccc1").unwrap_err();
    assert_eq!(error.span.map(|s| s.start), Some(0));
}

#[test]
fn the_writer_refuses_aromatic_bridges_with_the_bond_named() {
    // Two aromatic benzene rings joined by an aromatic bond, which no ring contains.
    let mut atoms = Vec::new();
    let mut bonds = Vec::new();
    for ring in 0..2 {
        for _ in 0..6 {
            atoms.push(element_atom("C"));
        }
        for i in 0..6 {
            bonds.push((ring * 6 + i, ring * 6 + (i + 1) % 6, BondOrder::Aromatic));
        }
    }
    bonds.push((0, 6, BondOrder::Aromatic));
    let error = write_recorded(&build(atoms.clone(), bonds.clone())).unwrap_err();
    assert_eq!(error.code, ErrorCode::AromaticBridgeNotWritable);
    assert_eq!(error.subject, Some(Subject::Bond(bond_id(13))));
    // Joined by a single bond it is biphenyl, and it round trips.
    bonds[12].2 = BondOrder::Single;
    assert_eq!(
        write_recorded(&build(atoms, bonds)).unwrap().text,
        "c1(ccccc1)-c1ccccc1"
    );
}

#[test]
fn the_writer_names_the_real_cause_when_aromaticity_is_unsupported() {
    // Five aromatic bonds in a five-membered carbon ring: the model does not support it.
    let atoms = vec![element_atom("C"); 5];
    let bonds = (0..5)
        .map(|i| (i, (i + 1) % 5, BondOrder::Aromatic))
        .collect();
    let error = write_recorded(&build(atoms, bonds)).unwrap_err();
    assert_eq!(error.code, ErrorCode::AromaticityUnsupported);
    assert!(error.reason.is_some());
    // An atom that cannot be formatted for another reason is reported for that reason, not
    // as an aromaticity failure.
    let mut unknown = element_atom("C");
    unknown.hydrogens = Hydrogens::Unknown;
    let atoms = vec![unknown, element_atom("C")];
    let bonds = vec![(0, 1, BondOrder::Aromatic)];
    assert_eq!(
        write_recorded(&build(atoms, bonds)).unwrap_err().code,
        ErrorCode::HydrogensUnknownNotWritable
    );
}

#[test]
fn a_ring_label_closing_at_an_atom_is_free_for_the_rings_that_atom_opens() {
    assert_eq!(spelled("C1CCC11CCCC1"), "C1CCC11CCCC1");
}

/// A hub bonded to a chain: the hub opens one ring per chain atom after the first.
fn hub_and_chain(chain: usize, tail: bool) -> MoleculeGraph {
    let mut atoms = vec![element_atom("C"); 1 + chain];
    let mut bonds = Vec::new();
    for i in 1..=chain {
        bonds.push((0, i, BondOrder::Single));
        if i > 1 {
            bonds.push((i - 1, i, BondOrder::Single));
        }
    }
    if tail {
        // An extra atom bonded to the second chain atom and the last: a ring that atom 2
        // opens at the moment it closes the hub's ring to it.
        atoms.push(element_atom("C"));
        let extra = atoms.len() - 1;
        bonds.push((2, extra, BondOrder::Single));
        bonds.push((extra, chain, BondOrder::Single));
    }
    build(atoms, bonds)
}

#[test]
fn all_ring_labels_can_be_open_at_once_and_one_more_is_a_limit() {
    // The hub opens `chain - 1` rings at once; MAX_OPEN_RINGS is exactly enough.
    let written = write_recorded(&hub_and_chain(MAX_OPEN_RINGS + 1, false)).unwrap();
    assert!(
        written.text.starts_with("C%99") || written.text.contains("%99"),
        "{}",
        written.text
    );
    assert!(
        written.text.contains('0'),
        "label 0 is the last one used: {}",
        written.text
    );
    assert_eq!(
        read(&written.text).unwrap().bonds.len(),
        2 * MAX_OPEN_RINGS + 1
    );
    let error = write_recorded(&hub_and_chain(MAX_OPEN_RINGS + 2, false)).unwrap_err();
    assert_eq!(error.code, ErrorCode::RingLabelLimit);
    assert_eq!(error.class(), ErrorClass::Limit);
}

#[test]
fn a_label_that_closes_is_reused_at_the_label_limit() {
    // With every label in use, atom 2 closes the hub's ring and opens one to the extra atom.
    // That needs the closing label to be freed before the opening one is allocated.
    let graph = hub_and_chain(MAX_OPEN_RINGS + 1, true);
    let written = write_recorded(&graph).unwrap();
    assert_eq!(
        read(&written.text).unwrap().bonds.len(),
        2 * MAX_OPEN_RINGS + 3
    );
}

#[test]
fn a_branch_nesting_the_reader_would_refuse_is_refused_by_the_writer() {
    // A comb: every backbone atom carries a methyl, so each methyl is a branch and the
    // backbone continues. Depth stays 1. Nesting needs branches on branches: a binary tree.
    let mut atoms = vec![element_atom("C")];
    let mut bonds = Vec::new();
    // A "caterpillar" whose first child is always the deeper subtree: each level is
    // written as a branch containing the rest.
    let levels = MAX_BRANCH_DEPTH + 3;
    let mut parent = 0;
    for _ in 0..levels {
        // A deeper child (lower index, written first as a branch) and a leaf written last.
        let deep = atoms.len();
        atoms.push(element_atom("C"));
        bonds.push((parent, deep, BondOrder::Single));
        let leaf = atoms.len();
        atoms.push(element_atom("C"));
        bonds.push((parent, leaf, BondOrder::Single));
        parent = deep;
    }
    let error = write_recorded(&build(atoms, bonds)).unwrap_err();
    assert_eq!(error.code, ErrorCode::BranchDepthLimit);
    assert_eq!(error.class(), ErrorClass::Limit);
}

#[test]
fn a_structure_over_the_command_limit_is_refused() {
    let atoms = vec![element_atom("C"); MAX_COMMANDS / 2 + 1];
    let bonds = (1..atoms.len())
        .map(|i| (i - 1, i, BondOrder::Single))
        .collect();
    assert_eq!(
        write_recorded(&build(atoms, bonds)).unwrap_err().code,
        ErrorCode::CommandLimit
    );
    assert_eq!(
        write_canonical(&build(vec![element_atom("C"); MAX_COMMANDS + 1], vec![]))
            .unwrap_err()
            .code,
        ErrorCode::CommandLimit
    );
}

#[test]
fn text_longer_than_the_reader_accepts_is_refused() {
    // Isolated isotopic cations: ten bytes each, within the command limit but over the text limit.
    let atoms: Vec<Atom> = (0..MAX_COMMANDS)
        .map(|_| {
            let mut atom = element_atom("C");
            atom.isotope = Some(13);
            atom.charge = 1;
            atom
        })
        .collect();
    let error = write_recorded(&build(atoms, vec![])).unwrap_err();
    assert_eq!(error.code, ErrorCode::OutputTooLong);
    assert_eq!(error.class(), ErrorClass::Limit);
}

// ---------------------------------------------------------------------------------------------
// Canonical form

#[test]
fn canonical_text_is_stable_under_spelling() {
    let groups: &[&[&str]] = &[
        &[
            "CCO",
            "OCC",
            "C(O)C",
            "C(C)O",
            "[H]OCC",
            "C([H])([H])([H])C([H])([H])O[H]",
        ],
        &["c1ccccc1", "C1=CC=CC=C1", "C1=CC=CC=C1", "c1ccc[cH]c1"],
        &[
            "Cc1ccccc1",
            "c1ccccc1C",
            "c1ccc(C)cc1",
            "CC1=CC=CC=C1",
            "C1(C)=CC=CC=C1",
        ],
        &[
            "c1cc[nH]c1",
            "[nH]1cccc1",
            "C1=CNC=C1",
            "C1=C[NH]C=C1",
            "c1ccn([H])c1",
        ],
        &["OC(C)=O", "CC(=O)O", "O=C(C)O", "C(=O)(O)C", "CC(O)=O"],
        &["C1CC1C", "CC1CC1", "C1(C)CC1"],
        &["[Na+].[Cl-]", "[Cl-].[Na+]"],
    ];
    for group in groups {
        let first = canonical_or_refusal(group[0]);
        for text in &group[1..] {
            assert_eq!(
                canonical_or_refusal(text),
                first,
                "{} vs {}",
                group[0],
                text
            );
        }
    }
}

/// The canonical text, or the refusal code: spellings must agree on one of the two.
fn canonical_or_refusal(text: &str) -> Result<String, ErrorCode> {
    write_canonical(&graph_of(text))
        .map(|w| w.text)
        .map_err(|e| e.code)
}

#[test]
fn canonical_output_is_a_fixpoint_and_names_its_model() {
    assert_eq!(CANONICAL_MODEL_ID, "chemifree-smiles-canonical-1");
    assert_eq!(CANONICAL_GRAPH_MODEL, "chemifree-chemical-graph-2");
    for text in [
        "CCO",
        "C1=CC=CC=C1",
        "c1cc[nH]c1",
        "C1CC2CCC1C2",
        "CC1=CCC2CC1C2(C)C",
        "[NH4+]",
    ] {
        let once = canonical(text);
        assert_eq!(canonical(&once), once, "{text}");
    }
}

#[test]
fn canonical_folds_explicit_hydrogens_and_lists_them_apart_from_the_order() {
    let graph = graph_of("[H]C([H])([H])O[H]");
    let written = write_canonical(&graph).unwrap();
    assert_eq!(written.text, "CO");
    assert_eq!(
        written.order.len(),
        2,
        "folded hydrogens are not in the atom order"
    );
    assert_eq!(written.folded_hydrogens.len(), 4);
    for (hydrogen, heavy) in &written.folded_hydrogens {
        assert!(!written.order.contains(hydrogen));
        assert!(written.order.contains(heavy));
    }
    // A labelled hydrogen, a charged one and H2 stay vertices.
    assert_eq!(canonical("[2H]O[2H]"), "[2H]O[2H]");
    assert_eq!(canonical("[H][H]"), "[H][H]");
    assert!(
        write_canonical(&graph_of("[H][H]"))
            .unwrap()
            .folded_hydrogens
            .is_empty()
    );
}

#[test]
fn canonical_reports_each_representation_change() {
    // Kekulé bonds written as aromatic.
    let benzene = write_canonical(&graph_of("C1=CC=CC=C1")).unwrap();
    assert_eq!(benzene.kekule_bonds_aromatized.len(), 6);
    assert_eq!(
        write_canonical(&graph_of("c1ccccc1"))
            .unwrap()
            .kekule_bonds_aromatized
            .len(),
        0
    );
    // Stated hydrogens that become inferred...
    let stated = write_canonical(&graph_of("[CH3][CH3]")).unwrap();
    assert_eq!(stated.text, "CC");
    assert_eq!(stated.hydrogen_state_changes.len(), 2);
    // ...and inferred ones that become stated, where a bracket needs the count.
    let mut ammonium = element_atom("N");
    ammonium.charge = 1;
    let written = write_canonical(&build(vec![ammonium], vec![])).unwrap();
    assert_eq!(
        (written.text.as_str(), written.hydrogen_state_changes.len()),
        ("[NH4+]", 1)
    );
    // Nothing is reported where nothing changed.
    let plain = write_canonical(&graph_of("c1cc[nH]c1")).unwrap();
    assert!(plain.hydrogen_state_changes.is_empty() && plain.kekule_bonds_aromatized.is_empty());
}

#[test]
fn canonical_refuses_what_the_graph_identity_model_cannot_decide() {
    for text in [
        "O=C1CCCCC1",
        "C1CCC2=CC=CC=C2C1",
        "[CH]1C=CC=C1",
        "CN1(C)C=CC=C1",
    ] {
        let error = write_canonical(&graph_of(text)).unwrap_err();
        assert_eq!(error.code, ErrorCode::CanonicalUnsupported, "{text}");
        assert!(error.reason.is_some(), "{text}");
    }
}

// ---------------------------------------------------------------------------------------------
// Property tests

struct Entry {
    class: String,
    name: String,
    smiles: String,
}

fn corpus() -> Vec<Entry> {
    let value: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../docs/verification/aromaticity-corpus.json"
    ))
    .unwrap();
    let mut seen = std::collections::BTreeSet::new();
    let mut entries = Vec::new();
    for item in value.as_array().unwrap() {
        let smiles = item["smiles"].as_str().unwrap().to_owned();
        // The corpus lists each molecule in two spellings; the SMILES is the same.
        if seen.insert(smiles.clone()) {
            entries.push(Entry {
                class: item["class"].as_str().unwrap().to_owned(),
                name: item["name"].as_str().unwrap().to_owned(),
                smiles,
            });
        }
    }
    // Species the corpus does not list: isotopes, multi-component, quaternary, spiro, strain.
    for (class, name, smiles) in [
        ("isotopic", "heavy water", "[2H]O[2H]"),
        ("isotopic", "carbon-13 methanol", "[13CH3]O"),
        ("multi-component", "sodium chloride", "[Na+].[Cl-]"),
        ("multi-component", "ethanol and benzene", "CCO.c1ccccc1"),
        ("quaternary", "neopentane", "CC(C)(C)C"),
        ("quaternary", "tetramethylammonium", "C[N+](C)(C)C"),
        ("quaternary", "ammonium", "[NH4+]"),
        ("spiro", "spiropentane", "C1CC11CC1"),
        ("bridged", "alpha-pinene", "CC1=CCC2CC1C2(C)C"),
        ("bridged", "norbornene", "C1=CC2CCC1C2"),
        ("charged", "acetate", "CC(=O)[O-]"),
        ("charged", "tropylium", "c1cc[cH+]ccc1"),
        (
            "heteroaromatic",
            "caffeine core: N-methylimidazole",
            "Cn1ccnc1",
        ),
        ("two rings", "biphenyl", "c1ccccc1-c1ccccc1"),
    ] {
        if seen.insert(smiles.to_owned()) {
            entries.push(Entry {
                class: class.to_owned(),
                name: name.to_owned(),
                smiles: smiles.to_owned(),
            });
        }
    }
    entries
}

#[test]
fn corpus_molecules_agree_across_spellings_permutations_and_round_trips() {
    let corpus = corpus();
    assert!(corpus.len() > 100, "{}", corpus.len());
    let mut classes = std::collections::BTreeSet::new();
    let mut refused = 0;
    let mut written = 0;
    let mut rng = Rng(0x00c0_ffee_d00d_cafe);
    for entry in &corpus {
        classes.insert(entry.class.as_str());
        let label = format!("{} / {} / {}", entry.class, entry.name, entry.smiles);
        let Ok(read) = read(&entry.smiles) else {
            // The reader refuses an aromatic spelling the model does not support. Its Kekulé
            // spelling, if it reads, must then be refused by the canonical writer too.
            refused += 1;
            continue;
        };
        let graph = read.graph();
        let result = write_canonical(&graph).map(|w| w.text).map_err(|e| e.code);

        // Spellings: the Kekulé spelling of the same molecule.
        let has_aromatic = graph.bonds().any(|(_, b)| b.order == BondOrder::Aromatic);
        let mut spellings = vec![graph.clone()];
        if has_aromatic {
            spellings.push(kekule_spelling(&graph));
        }
        // Permutations of each spelling.
        let mut variants = Vec::new();
        for spelling in &spellings {
            variants.push(spelling.clone());
            for _ in 0..4 {
                variants.push(permuted(spelling, &mut rng));
            }
        }
        for variant in &variants {
            let again = write_canonical(variant).map(|w| w.text).map_err(|e| e.code);
            assert_eq!(again, result, "{label}: a spelling or permutation differs");
        }

        match &result {
            Ok(text) => {
                written += 1;
                // Fixpoint.
                let reread = read_or_panic(text, &label);
                assert_eq!(
                    write_canonical(&reread).map(|w| w.text).map_err(|e| e.code),
                    result,
                    "{label}: {text} is not a fixpoint"
                );
                // The canonical graph code of the re-read text is the original's.
                assert_eq!(
                    identity::canonicalize_chemical(&reread).unwrap().code,
                    identity::canonicalize_chemical(&graph).unwrap().code,
                    "{label}: {text} reads back as another molecule"
                );
            }
            Err(code) => {
                assert_eq!(*code, ErrorCode::CanonicalUnsupported, "{label}");
            }
        }

        // Recorded write: the string reads back as the same structure.
        for variant in &variants {
            match write_recorded(variant) {
                Ok(w) => {
                    let reread = read_or_panic(&w.text, &label);
                    let order: Vec<AtomId> = reread.atoms().map(|(id, _)| id).collect();
                    assert_eq!(
                        meaning(variant, &w.order),
                        meaning(&reread, &order),
                        "{label}: {} does not read back equal",
                        w.text
                    );
                }
                Err(error) => assert_ne!(error.code, ErrorCode::WriterInconsistent, "{label}"),
            }
        }
    }
    for class in [
        "acyclic",
        "saturated ring",
        "benzenoid",
        "monocyclic heteroaromatic",
        "fused heteroaromatic",
        "fused partially saturated",
        "bridged",
        "spiro",
        "charged",
        "isotopic",
        "multi-component",
        "quaternary",
    ] {
        assert!(classes.contains(class), "the corpus lacks {class}");
    }
    assert!(
        written > 60 && refused > 20,
        "written {written}, refused {refused}"
    );
}

fn read_or_panic(text: &str, label: &str) -> MoleculeGraph {
    read(text)
        .unwrap_or_else(|e| {
            panic!("{label}: the writer produced {text:?}, which does not read: {e:?}")
        })
        .graph()
}

/// A random molecule: a tree with extra bonds, random elements, charges, isotopes, hydrogen
/// states and bond kinds. Most are chemically odd; the writers must still either refuse with
/// a specific code or produce text the reader accepts.
fn random_graph(rng: &mut Rng) -> MoleculeGraph {
    const ELEMENTS: [&str; 12] = ["C", "C", "C", "N", "O", "S", "P", "F", "Cl", "Na", "H", "B"];
    let count = 1 + rng.below(14);
    let mut atoms = Vec::new();
    for _ in 0..count {
        let mut atom = element_atom(ELEMENTS[rng.below(ELEMENTS.len())]);
        if rng.below(5) == 0 {
            atom.charge = i8::try_from(rng.below(5)).unwrap() - 2;
        }
        if rng.below(6) == 0 {
            atom.isotope = Some(u16::try_from(1 + rng.below(250)).unwrap());
        }
        match rng.below(8) {
            0 => atom.hydrogens = Hydrogens::Specified(u8::try_from(rng.below(5)).unwrap()),
            1 => {
                atom.radical = [Radical::Doublet, Radical::Singlet, Radical::Unknown][rng.below(3)];
            }
            _ => {}
        }
        atoms.push(atom);
    }
    let mut bonds = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let order = |rng: &mut Rng| match rng.below(10) {
        0 => BondOrder::Double,
        1 => BondOrder::Triple,
        2 => BondOrder::Aromatic,
        _ => BondOrder::Single,
    };
    for i in 1..count {
        if rng.below(12) == 0 {
            continue;
        }
        let j = rng.below(i);
        seen.insert((j, i));
        let o = order(rng);
        bonds.push((j, i, o));
    }
    for _ in 0..rng.below(4) {
        if count < 3 {
            break;
        }
        let a = rng.below(count);
        let b = rng.below(count);
        let key = (a.min(b), a.max(b));
        if a != b && seen.insert(key) {
            let o = order(rng);
            bonds.push((key.0, key.1, o));
        }
    }
    build(atoms, bonds)
}

/// A random molecule that respects ordinary valences: atoms are attached only where valence
/// is free, rings close between atoms with free valence, and an aromatic ring is sometimes
/// attached. Most have a canonical form, so the canonical writer is exercised, not only refused.
fn random_valid_graph(rng: &mut Rng) -> MoleculeGraph {
    const KINDS: [(&str, usize); 7] = [
        ("C", 4),
        ("C", 4),
        ("C", 4),
        ("N", 3),
        ("O", 2),
        ("S", 2),
        ("Cl", 1),
    ];
    let count = 1 + rng.below(16);
    let mut atoms: Vec<Atom> = Vec::new();
    let mut free: Vec<usize> = Vec::new();
    let mut bonds: Vec<(usize, usize, BondOrder)> = Vec::new();
    let mut bonded = std::collections::BTreeSet::new();
    for i in 0..count {
        let (symbol, valence) = KINDS[rng.below(KINDS.len())];
        let mut atom = element_atom(symbol);
        if symbol == "C" && rng.below(8) == 0 {
            atom.isotope = Some(13);
        }
        atoms.push(atom);
        free.push(valence);
        let candidates: Vec<usize> = (0..i).filter(|&j| free[j] > 0).collect();
        if candidates.is_empty() {
            continue;
        }
        let parent = candidates[rng.below(candidates.len())];
        let wanted = if rng.below(6) == 0 {
            2 + rng.below(2)
        } else {
            1
        };
        let order = wanted.min(free[parent]).min(free[i]);
        free[parent] -= order;
        free[i] -= order;
        bonded.insert((parent, i));
        bonds.push((
            parent,
            i,
            match order {
                1 => BondOrder::Single,
                2 => BondOrder::Double,
                _ => BondOrder::Triple,
            },
        ));
    }
    for _ in 0..rng.below(3) {
        let open: Vec<usize> = (0..atoms.len()).filter(|&j| free[j] > 0).collect();
        if open.len() < 2 {
            break;
        }
        let (a, b) = (open[rng.below(open.len())], open[rng.below(open.len())]);
        let key = (a.min(b), a.max(b));
        if a != b && bonded.insert(key) {
            free[a] -= 1;
            free[b] -= 1;
            bonds.push((key.0, key.1, BondOrder::Single));
        }
    }
    if rng.below(4) == 0 {
        let base = atoms.len();
        for k in 0..6 {
            let symbol = if k == 3 && rng.below(3) == 0 {
                "N"
            } else {
                "C"
            };
            atoms.push(element_atom(symbol));
        }
        for k in 0..6 {
            bonds.push((base + k, base + (k + 1) % 6, BondOrder::Aromatic));
        }
        let open: Vec<usize> = (0..base).filter(|&j| free[j] > 0).collect();
        if !open.is_empty() {
            bonds.push((open[rng.below(open.len())], base, BondOrder::Single));
        }
    }
    build(atoms, bonds)
}

#[test]
fn random_molecules_write_to_text_the_reader_accepts_or_are_refused_specifically() {
    let mut rng = Rng(0x1234_5678_9abc_def1);
    let mut written = 0;
    let mut valid_canonical = 0;
    let mut refused = BTreeMap::<&str, usize>::new();
    for round in 0..8000 {
        let valid = round % 2 == 0;
        let graph = if valid {
            random_valid_graph(&mut rng)
        } else {
            random_graph(&mut rng)
        };
        for (name, result) in [
            ("recorded", write_recorded(&graph)),
            ("canonical", write_canonical(&graph)),
        ] {
            match result {
                Ok(w) => {
                    written += 1;
                    if valid && name == "canonical" {
                        valid_canonical += 1;
                        // Permuting the atoms cannot change the canonical text.
                        let again = write_canonical(&permuted(&graph, &mut rng)).unwrap();
                        assert_eq!(again.text, w.text, "round {round}");
                    }
                    assert!(
                        read(&w.text).is_ok(),
                        "round {round} {name}: {:?} is not readable",
                        w.text
                    );
                }
                Err(error) => {
                    assert_ne!(
                        error.code,
                        ErrorCode::WriterInconsistent,
                        "round {round} {name}"
                    );
                    assert!(
                        matches!(error.class(), ErrorClass::NotWritable | ErrorClass::Limit),
                        "round {round} {name}: {error:?}"
                    );
                    *refused.entry(error.code.code()).or_default() += 1;
                }
            }
        }
    }
    assert!(
        written > 4000,
        "only {written} strings were written ({refused:?})"
    );
    assert!(
        valid_canonical > 1500,
        "only {valid_canonical} valid molecules had a canonical form"
    );
    assert!(refused.len() >= 4, "refusals were not varied: {refused:?}");
}

// ---------------------------------------------------------------------------------------------
// The read-back check

fn methanol_vertices(
    expected_carbon: Option<u16>,
) -> (Vec<super::write::Vertex>, Vec<super::write::Edge>) {
    use super::write::{Edge, Kind, bare_vertex};
    let carbon = Element::from_symbol("C").unwrap();
    let oxygen = Element::from_symbol("O").unwrap();
    (
        vec![
            bare_vertex(atom_id(1), carbon, false, expected_carbon, false),
            bare_vertex(atom_id(2), oxygen, false, Some(1), false),
        ],
        vec![Edge {
            id: bond_id(1),
            a: 0,
            b: 1,
            kind: Kind::Single,
        }],
    )
}

#[test]
fn the_read_back_compares_the_hydrogens_of_bare_atoms_too() {
    use super::write::{Roots, emit, read_back_mismatches, render};
    // Intended hydrogens that match what the reader infers: nothing to report.
    let (vertices, edges) = methanol_vertices(Some(3));
    let (text, emitted) = emit(&vertices, &edges, Roots::First).unwrap();
    assert_eq!(text, "CO");
    assert!(
        read_back_mismatches(&text, &vertices, &edges, &emitted)
            .unwrap()
            .is_empty()
    );
    // A wrong bare decision (two hydrogens meant, three inferred) is found...
    let (mut vertices, edges) = methanol_vertices(Some(2));
    let (text, emitted) = emit(&vertices, &edges, Roots::First).unwrap();
    assert_eq!(
        read_back_mismatches(&text, &vertices, &edges, &emitted).unwrap(),
        vec![0]
    );
    // ...and is never written as if it were right: bracketing it changes the radical state the
    // reader gives back, which is a writer fault, not a result.
    let error = render(&mut vertices, &edges, Roots::First).unwrap_err();
    assert_eq!(error.code, ErrorCode::WriterInconsistent);
    // Where the model cannot resolve the hydrogens, nothing is compared.
    let (vertices, edges) = methanol_vertices(None);
    let (text, emitted) = emit(&vertices, &edges, Roots::First).unwrap();
    assert!(
        read_back_mismatches(&text, &vertices, &edges, &emitted)
            .unwrap()
            .is_empty()
    );
}
