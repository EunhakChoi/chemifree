//! The recorded-order writer, the refusals both writers share, and the round-trip invariant:
//! every string a writer returns is read back by the same reader, or the write is refused with
//! a specific code.

mod smiles_common;

use chemifree_application::smiles_codec::{SmilesError, SmilesStyle, WriteChange};
use chemifree_domain::document::{Atom, AtomId, Bond, BondOrder, MAX_ISOTOPE_MASS_NUMBER};
use chemifree_domain::element::Element;
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::states::{AtomKind, AttachmentPoints, Hydrogens, Radical};
use chemifree_providers::smiles::{Code, MAX_INPUT_BYTES, MAX_OPEN_RINGS};
use smiles_common::{Rng, atom_id, bond_id, graph, read, write};

fn atom(symbol: &str) -> Atom {
    Atom::new(AtomKind::Element(Element::from_symbol(symbol).unwrap()))
}

fn build(atoms: Vec<Atom>, bonds: &[(usize, usize, BondOrder)]) -> MoleculeGraph {
    MoleculeGraph::from_parts(
        atoms.into_iter().enumerate().map(|(i, a)| (atom_id(i), a)).collect(),
        bonds
            .iter()
            .enumerate()
            .map(|(i, (a, b, order))| {
                (bond_id(i), Bond { begin: atom_id(*a), end: atom_id(*b), order: order.clone() })
            })
            .collect(),
    )
}

fn recorded(text: &str) -> String {
    let read = read(text).unwrap();
    write(&graph(&read), SmilesStyle::Recorded).unwrap().text
}

fn refusal(graph: &MoleculeGraph, style: SmilesStyle) -> SmilesError {
    write(graph, style).expect_err("the write must be refused")
}

#[test]
fn recorded_writes_follow_the_drawing_order() {
    for (input, expected) in [
        ("CCO", "CCO"),
        ("OCC", "OCC"),
        ("C1=CC=CC=C1", "C1=CC=CC=C1"),
        ("c1ccccc1c1ccccc1", "c1ccccc1-c1ccccc1"),
        ("Oc1ccccc1.NCCO", "Oc1ccccc1.NCCO"),
        ("C12(CCCCC1)CCCCC2", "C12(CCCCC1)CCCCC2"),
        ("[Na+].[Cl-]", "[Na+].[Cl-]"),
        ("[2H]C([2H])([2H])[2H]", "[2H]C([2H])([2H])[2H]"),
        ("[CH3]", "[CH3]"),
        ("[CH4]", "[CH4]"),
        ("C%12CCCCC%12", "C1CCCCC1"),
        ("c1cc[nH]c1", "c1cc[nH]c1"),
        ("CC(C)(C)C", "CC(C)(C)C"),
        ("C=1CCCCC1", "C=1CCCCC1"),
    ] {
        let written = recorded(input);
        // Ring bonds put their symbol at the opening digit.
        let expected = if input == "C=1CCCCC1" { "C=1CCCCC1" } else { expected };
        assert_eq!(written, expected, "{input}");
    }
}

#[test]
fn every_returned_text_reads_back_to_the_graph_written() {
    for input in [
        "CCO", "c1ccccc1", "C1=CC=CC=C1", "[NH4+]", "CC(=O)[O-]", "[13CH4]", "[Na+].[Cl-]",
        "c1ccc2ccccc2c1", "OC(=O)c1ccccc1O", "C1CC2CCC1C2", "[H][H]", "[H]C([H])([H])[H]",
    ] {
        let source = read(input).unwrap();
        let graph = graph(&source);
        let written = write(&graph, SmilesStyle::Recorded).unwrap();
        let back = read(&written.text).unwrap();
        assert_eq!(back.atoms.len(), source.atoms.len(), "{input}");
        assert_eq!(back.bonds.len(), source.bonds.len(), "{input}");
        assert_eq!(written.atom_order.len(), source.atoms.len(), "{input}");
        for (position, id) in written.atom_order.iter().enumerate() {
            let original = &source.atoms[usize::try_from(id.serial()).unwrap() - 1];
            let reread = &back.atoms[position];
            assert_eq!(reread.atom.kind, original.atom.kind, "{input}");
            assert_eq!(reread.atom.charge, original.atom.charge, "{input}");
            assert_eq!(reread.atom.isotope, original.atom.isotope, "{input}");
            assert_eq!(reread.atom.radical, original.atom.radical, "{input}");
            assert_eq!(reread.hydrogens, original.hydrogens, "{input}");
        }
    }
}

#[test]
fn inferred_hydrogens_inside_brackets_are_stated_from_the_valence_model() {
    // A drawn ammonium: charged, so it needs brackets, and its hydrogens were inferred.
    let mut nitrogen = atom("N");
    nitrogen.charge = 1;
    let graph = build(
        vec![nitrogen, atom("C"), atom("C")],
        &[(0, 1, BondOrder::Single), (0, 2, BondOrder::Single)],
    );
    let written = write(&graph, SmilesStyle::Recorded).unwrap();
    assert_eq!(written.text, "[NH2+](C)C");
    assert!(
        written
            .changes
            .contains(&WriteChange::InferredHydrogensStated(vec![atom_id(0)])),
        "{:?}",
        written.changes
    );
    // A drawn hydroxide: a charged oxygen with no neighbours has none to state.
    let mut oxygen = atom("O");
    oxygen.charge = -1;
    let hydroxide = build(vec![oxygen], &[]);
    assert_eq!(write(&hydroxide, SmilesStyle::Recorded).unwrap().text, "[OH-]");
    // A stated count is kept, even when it equals the inferred one.
    let mut stated = atom("C");
    stated.hydrogens = Hydrogens::Specified(4);
    let methane = build(vec![stated], &[]);
    assert_eq!(write(&methane, SmilesStyle::Recorded).unwrap().text, "[CH4]");
}

#[test]
fn ions_and_metals_drawn_with_defaults_are_written_with_consent_losses() {
    let mut sodium = atom("Na");
    sodium.charge = 1;
    let graph = build(vec![sodium, atom("Fe")], &[]);
    let written = write(&graph, SmilesStyle::Recorded).unwrap();
    assert_eq!(written.text, "[Na+].[Fe]");
    assert!(written.changes.contains(&WriteChange::HydrogensAssumed(vec![atom_id(0), atom_id(1)])));
    assert!(written.changes.contains(&WriteChange::RadicalAssumed(vec![atom_id(0), atom_id(1)])));
    // A stated radical on such an atom cannot be written: reading gives unknown.
    let mut radical = atom("Fe");
    radical.radical = Radical::Doublet;
    let error = refusal(&build(vec![radical], &[]), SmilesStyle::Recorded);
    assert_eq!(error.code, "radical_not_representable");
}

#[test]
fn bare_atoms_are_checked_for_their_radical_too() {
    // A lone hydrogen atom drawn without a radical reads back as a doublet.
    let error = refusal(&build(vec![atom("H")], &[]), SmilesStyle::Recorded);
    assert_eq!(error.code, "radical_not_representable");
    // A carbene drawn as a triplet is not distinguishable from a singlet in SMILES.
    let mut carbene = atom("C");
    carbene.hydrogens = Hydrogens::Specified(2);
    carbene.radical = Radical::Triplet;
    assert_eq!(
        refusal(&build(vec![carbene], &[]), SmilesStyle::Recorded).code,
        "radical_not_representable"
    );
}

#[test]
fn unwritable_graphs_are_refused_with_their_own_code() {
    let cases: Vec<(&str, MoleculeGraph, &str)> = vec![
        ("empty", build(vec![], &[]), "empty_graph"),
        ("isotope zero", build(vec![{ let mut a = atom("C"); a.isotope = Some(0); a }], &[]), "isotope_not_writable"),
        (
            "isotope above 999",
            build(vec![{ let mut a = atom("C"); a.isotope = Some(MAX_ISOTOPE_MASS_NUMBER + 1); a }], &[]),
            "isotope_not_writable",
        ),
        ("charge above the domain", build(vec![{ let mut a = atom("Fe"); a.charge = 9; a }], &[]), "charge_not_writable"),
        ("charge below the domain", build(vec![{ let mut a = atom("Fe"); a.charge = -9; a }], &[]), "charge_not_writable"),
        (
            "unknown hydrogens",
            build(vec![{ let mut a = atom("C"); a.hydrogens = Hydrogens::Unknown; a }], &[]),
            "hydrogens_unknown",
        ),
        (
            "query atom",
            build(vec![Atom::new(AtomKind::Query(chemifree_domain::states::QueryAtom::Any))], &[]),
            "query_not_writable",
        ),
        ("dative bond", build(vec![atom("N"), atom("B")], &[(0, 1, BondOrder::Dative)]), "bond_not_writable"),
        ("unknown bond", build(vec![atom("C"), atom("C")], &[(0, 1, BondOrder::Unknown)]), "bond_not_writable"),
        // An aromatic bond outside every ring (a bridge) or on a lone pair of atoms.
        ("aromatic bridge", build(vec![atom("C"), atom("C")], &[(0, 1, BondOrder::Aromatic)]), "aromatic_unsupported"),
        (
            "aromatic bond in a non-aromatic ring",
            build(
                vec![atom("C"), atom("C"), atom("C")],
                &[(0, 1, BondOrder::Aromatic), (1, 2, BondOrder::Single), (2, 0, BondOrder::Single)],
            ),
            "aromatic_unsupported",
        ),
    ];
    for (name, graph, code) in cases {
        for style in [SmilesStyle::Recorded, SmilesStyle::Canonical] {
            let error = write(&graph, style).expect_err(name);
            let expected = if style == SmilesStyle::Canonical && code == "aromatic_unsupported" {
                "canonical_unsupported"
            } else {
                code
            };
            assert_eq!(error.code, expected, "{name} {style:?}: {}", error.message);
        }
    }
}

#[test]
fn more_than_nine_hydrogens_cannot_be_written() {
    let mut carbon = atom("C");
    carbon.hydrogens = Hydrogens::Specified(12);
    let error = refusal(&build(vec![carbon], &[]), SmilesStyle::Recorded);
    assert_eq!(error.code, "hydrogens_not_representable");
    assert_eq!(error.detail, Some("hydrogen_count_above_nine"));
}

#[test]
fn the_real_cause_of_a_refusal_is_reported() {
    // An unsupported aromatic ring is named before any atom is formatted.
    let ring = build(
        vec![atom("C"), atom("C"), atom("C")],
        &[(0, 1, BondOrder::Aromatic), (1, 2, BondOrder::Aromatic), (2, 0, BondOrder::Aromatic)],
    );
    let error = refusal(&ring, SmilesStyle::Recorded);
    assert_eq!(error.code, "aromatic_unsupported");
    assert!(error.detail.is_some(), "the model's own reason is carried: {error:?}");
    // An unusual valence has a reason of its own, not a generic mismatch.
    let mut oxygen = atom("O");
    oxygen.charge = 1;
    let crowded = build(
        vec![oxygen, atom("C"), atom("C"), atom("C"), atom("C")],
        &[(0, 1, BondOrder::Single), (0, 2, BondOrder::Single), (0, 3, BondOrder::Single), (0, 4, BondOrder::Single)],
    );
    let error = refusal(&crowded, SmilesStyle::Recorded);
    assert_eq!(error.code, "radical_not_representable");
    assert_eq!(error.detail, Some("unusual_valence"));
}

#[test]
fn attachment_points_are_dropped_and_reported() {
    let mut anchored = atom("C");
    anchored.attachment_points = AttachmentPoints::new(&[1, 2]).unwrap();
    let graph = build(vec![anchored, atom("C")], &[(0, 1, BondOrder::Single)]);
    for style in [SmilesStyle::Recorded, SmilesStyle::Canonical] {
        let written = write(&graph, style).unwrap();
        assert!(
            written.changes.contains(&WriteChange::AttachmentPoints(vec![atom_id(0)])),
            "{style:?}: {:?}",
            written.changes
        );
    }
}

#[test]
fn text_beyond_the_readers_size_is_refused_by_the_writer() {
    // 500 isolated atoms are 500 commands, within the edit limit; their text is not within
    // the byte limit.
    let atoms: Vec<Atom> = (0..500)
        .map(|_| {
            let mut a = atom("Fe");
            a.isotope = Some(999);
            a.charge = 8;
            a
        })
        .collect();
    let error = refusal(&build(atoms, &[]), SmilesStyle::Recorded);
    assert_eq!(error.code, "text_too_long");
    assert!(MAX_INPUT_BYTES < 500 * "[999Fe+8]".len());
    // One command too many.
    let too_many = build((0..501).map(|_| atom("C")).collect(), &[]);
    assert_eq!(refusal(&too_many, SmilesStyle::Recorded).code, "command_limit");
}

/// A path of atoms with `chords` extra bonds from atom 0 to atoms 2.. (all open at atom 0).
fn hub(chords: usize, extra: Option<(usize, usize)>) -> MoleculeGraph {
    let last = extra.map_or(chords + 2, |(_, to)| to);
    let mut bonds: Vec<(usize, usize, BondOrder)> = (0..last).map(|i| (i, i + 1, BondOrder::Single)).collect();
    bonds.extend((2..2 + chords).map(|k| (0, k, BondOrder::Single)));
    bonds.extend(extra.map(|(from, to)| (from, to, BondOrder::Single)));
    build((0..=last).map(|_| atom("C")).collect(), &bonds)
}

#[test]
fn ring_labels_are_limited_and_a_closing_label_is_free_for_the_next_opening() {
    let at_limit = write(&hub(MAX_OPEN_RINGS, None), SmilesStyle::Recorded).unwrap();
    assert!(read(&at_limit.text).is_ok());
    let error = refusal(&hub(MAX_OPEN_RINGS + 1, None), SmilesStyle::Recorded);
    assert_eq!(error.code, "ring_label_limit");
    // Atom 2 closes a label and opens one while all labels are in use: only the freed one fits.
    let graph = hub(MAX_OPEN_RINGS - 1, Some((2, MAX_OPEN_RINGS + 4)));
    let written = write(&graph, SmilesStyle::Recorded).expect("the freed label is reused");
    let back = read(&written.text).unwrap();
    assert_eq!(back.bonds.len(), graph.bonds().len());
}

// ---- The round-trip property over random molecules ----

fn random_graph(rng: &mut Rng) -> MoleculeGraph {
    const SYMBOLS: [&str; 15] = [
        "C", "C", "C", "N", "O", "S", "P", "F", "Cl", "Br", "I", "B", "Si", "Na", "H",
    ];
    let count = 1 + rng.below(10);
    let mut atoms: Vec<Atom> = (0..count)
        .map(|_| {
            let mut a = atom(SYMBOLS[rng.below(SYMBOLS.len())]);
            if rng.below(5) == 0 {
                a.charge = i8::try_from(rng.below(7)).unwrap() - 3;
            }
            if rng.below(8) == 0 {
                a.isotope = Some(u16::try_from(rng.below(30)).unwrap());
            }
            a.hydrogens = match rng.below(6) {
                0 => Hydrogens::Specified(u8::try_from(rng.below(6)).unwrap()),
                1 if rng.below(5) == 0 => Hydrogens::Unknown,
                _ => Hydrogens::Inferred,
            };
            if rng.below(10) == 0 {
                a.radical = [Radical::Doublet, Radical::Singlet, Radical::Triplet, Radical::Unknown][rng.below(4)];
            }
            if rng.below(20) == 0 {
                a.attachment_points = AttachmentPoints::new(&[1]).unwrap();
            }
            a
        })
        .collect();
    let mut bonds: Vec<(usize, usize, BondOrder)> = Vec::new();
    let order = |rng: &mut Rng| match rng.below(12) {
        0..=5 => BondOrder::Single,
        6..=8 => BondOrder::Double,
        9 => BondOrder::Triple,
        10 => BondOrder::Aromatic,
        _ if rng.below(2) == 0 => BondOrder::Dative,
        _ => BondOrder::Unknown,
    };
    for i in 1..count {
        bonds.push((rng.below(i), i, order(rng)));
    }
    for _ in 0..rng.below(4) {
        let (a, b) = (rng.below(count), rng.below(count));
        let key = (a.min(b), a.max(b));
        if a != b && !bonds.iter().any(|(x, y, _)| (*x.min(y), *x.max(y)) == key) {
            bonds.push((a, b, order(rng)));
        }
    }
    // Sometimes a benzene-like ring of aromatic bonds, to reach the aromatic writers.
    if rng.below(4) == 0 && count >= 6 {
        for atom in atoms.iter_mut().take(6) {
            *atom = self::atom("C");
        }
        bonds.retain(|(a, b, _)| *a >= 6 || *b >= 6);
        bonds.retain(|(a, b, _)| !(*a < 6 && *b < 6));
        for i in 0..6 {
            bonds.push((i, (i + 1) % 6, if rng.below(2) == 0 { BondOrder::Aromatic } else if i % 2 == 0 { BondOrder::Double } else { BondOrder::Single }));
        }
    }
    build(atoms, &bonds)
}

#[test]
fn random_molecules_write_then_read_or_are_refused_with_a_specific_code() {
    let mut rng = Rng(0xC0DE_0021);
    let mut written = [0_usize; 2];
    let mut refused = std::collections::BTreeMap::<String, usize>::new();
    for round in 0..6000 {
        let molecule = random_graph(&mut rng);
        for (slot, style) in [SmilesStyle::Recorded, SmilesStyle::Canonical].into_iter().enumerate() {
            match write(&molecule, style) {
                Ok(output) => {
                    written[slot] += 1;
                    let back = read(&output.text).unwrap_or_else(|error| {
                        panic!("round {round} {style:?}: {:?} is not read back: {error:?}", output.text)
                    });
                    assert_eq!(
                        back.atoms.len() + output.folded_hydrogens.len(),
                        molecule.atoms().len(),
                        "round {round} {style:?} {:?}",
                        output.text
                    );
                    assert_eq!(back.atoms.len(), output.atom_order.len());
                }
                Err(error) => {
                    assert_ne!(
                        error.code, "round_trip_failed",
                        "round {round} {style:?}: the writer produced text its reader does not accept: {error:?}"
                    );
                    assert!(
                        Code::ALL.iter().any(|code| code.as_str() == error.code),
                        "unknown code {}",
                        error.code
                    );
                    *refused.entry(error.code.to_owned()).or_default() += 1;
                }
            }
        }
    }
    assert!(written[0] > 500 && written[1] > 100, "both writers are exercised: {written:?}");
    assert!(refused.len() >= 5, "several distinct refusals are exercised: {refused:?}");
}

#[test]
fn a_written_atom_id_is_the_graph_atom() {
    // atom_order names the document's atoms, not read positions.
    let source = read("OCC").unwrap();
    let graph = graph(&source);
    let written = write(&graph, SmilesStyle::Recorded).unwrap();
    let expected: Vec<AtomId> = (0..3).map(atom_id).collect();
    assert_eq!(written.atom_order, expected);
}
