//! Canonical SMILES (`chemifree-smiles-canonical-1`, bound to `chemifree-chemical-graph-2`)
//! over a broad corpus: permutation invariance, spelling agreement, the fixpoint and the
//! recorded round trip. A spelling set is either one text everywhere or refused everywhere.

mod smiles_common;

use chemifree_application::smiles_codec::{SmilesStyle, SmilesWrite, WriteChange};
use smiles_common::{Rng, graph, permuted, read, write};

/// What every spelling of a molecule must give.
#[derive(Debug)]
enum Outcome {
    Text,
    /// Refused with one of these codes and, where the model gives one, this reason. A
    /// spelling the reader refuses (an aromatic system outside the model) is refused there.
    Refused(&'static [&'static str], Option<&'static str>),
}

struct Entry {
    name: &'static str,
    spellings: &'static [&'static str],
    outcome: Outcome,
}

const fn text(name: &'static str, spellings: &'static [&'static str]) -> Entry {
    Entry { name, spellings, outcome: Outcome::Text }
}

const fn refused(
    name: &'static str,
    spellings: &'static [&'static str],
    codes: &'static [&'static str],
    reason: Option<&'static str>,
) -> Entry {
    Entry { name, spellings, outcome: Outcome::Refused(codes, reason) }
}

/// The aromatic spelling is refused by the reader, the Kekulé one by the canonical writer.
const UNREADABLE: &[&str] = &["aromatic_system_unsupported", "canonical_unsupported"];

const CORPUS: &[Entry] = &[
    // Acyclic.
    text("ethanol", &["CCO", "OCC", "C(O)C", "C(C)O"]),
    text("isobutane", &["CC(C)C", "C(C)(C)C", "C(C)(C)C"]),
    text("acrylonitrile", &["C=CC#N", "N#CC=C", "C(=C)C#N"]),
    text("acetic acid", &["CC(=O)O", "OC(C)=O", "O=C(C)O", "C(=O)(C)O"]),
    text("neopentane (quaternary)", &["CC(C)(C)C", "C(C)(C)(C)C", "C(C)(C)(C)C"]),
    text("methane, three spellings", &["C", "[CH4]", "[H]C([H])([H])[H]", "[CH4]"]),
    // Two hydrogen vertices and one hydrogen atom with a hydrogen are different graphs under
    // chemifree-chemical-graph-2 (H--H vertices stay distinct), so they are separate entries.
    text("hydrogen as two vertices", &["[H][H]"]),
    text("hydrogen as one atom with a hydrogen", &["[HH]"]),
    // Saturated rings, spiro and bridged.
    text("cyclohexane", &["C1CCCCC1", "C1CCCCC1", "C%10CCCCC%10", "C1CCCCC1"]),
    text("cyclopropane", &["C1CC1", "C1CC1"]),
    text("spiro[4.5]decane", &["C1CCC2(C1)CCCCC2", "C1CCCC2(C1)CCCC2", "C12(CCCC1)CCCCC2"]),
    text("norbornane", &["C1CC2CCC1C2", "C1C2CCC1CC2", "C12CCC(C1)CC2"]),
    text("norbornene", &["C1=CC2CCC1C2", "C1C2CCC1C=C2", "C2CC1C=CC2C1"]),
    text("alpha-pinene", &["CC1=CCC2CC1C2(C)C", "C2(C)(C)C1CC=C(C)C2C1", "CC1=CCC2CC1C2(C)C"]),
    text("adamantane", &["C1C2CC3CC1CC(C2)C3", "C1C2CC3CC1CC(C3)C2"]),
    // Heteroaromatics: Kekulé and aromatic spellings must agree.
    text("benzene", &["c1ccccc1", "C1=CC=CC=C1", "C1=CC=CC=C1", "c1ccccc1"]),
    text("pyridine", &["c1ccncc1", "C1=CC=NC=C1", "n1ccccc1", "C1=NC=CC=C1"]),
    text("pyrrole", &["c1cc[nH]c1", "C1=CNC=C1", "C1=C[NH]C=C1", "[nH]1cccc1"]),
    text("furan", &["c1ccoc1", "C1=COC=C1", "o1cccc1"]),
    text("thiophene", &["c1ccsc1", "C1=CSC=C1"]),
    text("imidazole", &["c1c[nH]cn1", "C1=CN=CN1", "n1cc[nH]c1"]),
    text("N-methylpyrrole", &["Cn1cccc1", "CN1C=CC=C1", "c1ccn(C)c1"]),
    text("pyridinium", &["[nH+]1ccccc1", "C1=CC=C[NH+]=C1"]),
    text("toluene", &["Cc1ccccc1", "c1ccccc1C", "CC1=CC=CC=C1", "C1=CC=CC=C1C"]),
    text("phenol", &["Oc1ccccc1", "c1ccc(O)cc1", "OC1=CC=CC=C1"]),
    text("biphenyl", &["c1ccccc1-c1ccccc1", "c1ccccc1c1ccccc1", "C1=CC=C(C=C1)C1=CC=CC=C1"]),
    // Fused carbocyclic aromatics.
    text("naphthalene", &["c1ccc2ccccc2c1", "C1=CC=C2C=CC=CC2=C1", "c1cccc2ccccc12", "c12ccccc1cccc2"]),
    text("anthracene", &["c1ccc2cc3ccccc3cc2c1", "C1=CC=C2C=C3C=CC=CC3=CC2=C1"]),
    // Fused systems the model refuses: heteroatom fused aromatic, and partially saturated.
    refused("indole", &["c1ccc2[nH]ccc2c1", "C1=CC=C2NC=CC2=C1"], UNREADABLE, None),
    refused("tetralin", &["C1CCc2ccccc2C1", "C1CCC2=CC=CC=C2C1"], UNREADABLE, None),
    refused("indane", &["C1Cc2ccccc2C1", "C1CC2=CC=CC=C2C1"], UNREADABLE, None),
    // Charged, isotopic and multi-component.
    text("ammonium", &["[NH4+]", "[N+]([H])([H])([H])[H]"]),
    text("acetate", &["CC(=O)[O-]", "[O-]C(C)=O", "CC([O-])=O"]),
    text("nitromethane", &["C[N+](=O)[O-]", "[O-][N+](=O)C", "O=[N+]([O-])C"]),
    text("deuterochloroform", &["[2H]C(Cl)(Cl)Cl", "ClC(Cl)(Cl)[2H]", "[2H]C(Cl)(Cl)Cl"]),
    text("carbon-13 methanol", &["[13CH3]O", "O[13CH3]", "[13CH3][OH]"]),
    text("ethanol and water", &["CCO.O", "O.CCO", "OCC.O", "O.OCC"]),
    text("benzene and ethanol", &["c1ccccc1.CCO", "CCO.c1ccccc1", "C1=CC=CC=C1.OCC"]),
    text("methyl radical", &["[CH3]", "[H][CH2]"]),
    refused("sodium chloride", &["[Na+].[Cl-]", "[Cl-].[Na+]"], &["canonical_unsupported"], Some("element_or_charge")),
    refused("carbene", &["[CH2]", "[CH2]"], &["canonical_unsupported"], Some("radical_state")),
    refused("iron", &["[Fe]"], &["canonical_unsupported"], Some("element_or_charge")),
];

fn canonical(text: &str) -> Result<SmilesWrite, (String, Option<&'static str>)> {
    let read = read(text).map_err(|error| (error.code.to_owned(), error.detail))?;
    write(&graph(&read), SmilesStyle::Canonical).map_err(|error| (error.code.to_owned(), error.detail))
}

#[test]
fn every_spelling_of_a_molecule_gives_the_same_canonical_result() {
    for entry in CORPUS {
        let results: Vec<_> = entry.spellings.iter().map(|s| (s, canonical(s))).collect();
        match &entry.outcome {
            Outcome::Text => {
                let first = results[0].1.as_ref().unwrap_or_else(|e| panic!("{}: {e:?}", entry.name));
                for (spelling, result) in &results {
                    let written = result.as_ref().unwrap_or_else(|e| panic!("{} {spelling}: {e:?}", entry.name));
                    assert_eq!(written.text, first.text, "{}: {spelling}", entry.name);
                }
            }
            Outcome::Refused(codes, reason) => {
                for (spelling, result) in &results {
                    let (found, detail) = result.as_ref().expect_err(&format!("{} {spelling}", entry.name)).clone();
                    assert!(codes.contains(&found.as_str()), "{} {spelling}: {found} {detail:?}", entry.name);
                    if let Some(reason) = reason {
                        assert_eq!(detail, Some(*reason), "{} {spelling}", entry.name);
                    }
                }
            }
        }
    }
}

#[test]
fn canonical_text_is_a_fixpoint() {
    for entry in CORPUS.iter().filter(|e| matches!(e.outcome, Outcome::Text)) {
        let first = canonical(entry.spellings[0]).unwrap();
        let again = canonical(&first.text).unwrap();
        assert_eq!(again.text, first.text, "{}", entry.name);
    }
}

#[test]
fn atom_order_permutations_do_not_change_the_text() {
    let mut rng = Rng(0x0021_0001);
    for entry in CORPUS.iter().filter(|e| matches!(e.outcome, Outcome::Text)) {
        let source = read(entry.spellings[0]).unwrap();
        let expected = canonical(entry.spellings[0]).unwrap().text;
        for round in 0..24 {
            let mut order: Vec<usize> = (0..source.atoms.len()).collect();
            rng.shuffle(&mut order);
            let shuffled = permuted(&source, &order, &mut rng, true);
            let written = write(&shuffled, SmilesStyle::Canonical)
                .unwrap_or_else(|e| panic!("{} round {round}: {e:?}", entry.name));
            assert_eq!(written.text, expected, "{} round {round} order {order:?}", entry.name);
        }
    }
}

#[test]
fn permuted_refusals_are_the_same_refusal() {
    let mut rng = Rng(0x0021_0002);
    for entry in CORPUS.iter().filter(|e| matches!(e.outcome, Outcome::Refused(..))) {
        let Some(readable) = entry.spellings.iter().find(|s| read(s).is_ok()) else { continue };
        let source = read(readable).unwrap();
        let expected = canonical(readable).unwrap_err();
        for _ in 0..12 {
            let mut order: Vec<usize> = (0..source.atoms.len()).collect();
            rng.shuffle(&mut order);
            let shuffled = permuted(&source, &order, &mut rng, true);
            let found = write(&shuffled, SmilesStyle::Canonical).unwrap_err();
            assert_eq!((found.code.to_owned(), found.detail), expected, "{}", entry.name);
        }
    }
}

#[test]
fn a_recorded_write_reads_back_to_an_equal_graph() {
    for entry in CORPUS {
        for spelling in entry.spellings {
            let Ok(source) = read(spelling) else { continue };
            let Ok(written) = write(&graph(&source), SmilesStyle::Recorded) else { continue };
            let back = read(&written.text).unwrap();
            assert_eq!(back.atoms.len(), source.atoms.len(), "{spelling}");
            // Compare bond sets in text order of the second string.
            let position = |id: chemifree_domain::document::AtomId| {
                written.atom_order.iter().position(|x| *x == id).unwrap()
            };
            let mut expected: Vec<_> = source
                .bonds
                .iter()
                .map(|b| {
                    let (x, y) = (position(smiles_common::atom_id(b.begin)), position(smiles_common::atom_id(b.end)));
                    (x.min(y), x.max(y), format!("{:?}", b.order))
                })
                .collect();
            let mut found: Vec<_> = back
                .bonds
                .iter()
                .map(|b| (b.begin.min(b.end), b.begin.max(b.end), format!("{:?}", b.order)))
                .collect();
            expected.sort();
            found.sort();
            assert_eq!(found, expected, "{spelling} -> {}", written.text);
            for (i, id) in written.atom_order.iter().enumerate() {
                let original = &source.atoms[usize::try_from(id.serial()).unwrap() - 1];
                assert_eq!(back.atoms[i].atom.radical, original.atom.radical, "{spelling}");
                assert_eq!(back.atoms[i].hydrogens, original.hydrogens, "{spelling}");
            }
        }
    }
}

#[test]
fn representation_changes_are_reported_and_folded_hydrogens_are_listed_apart() {
    // Explicit hydrogen vertices fold into carbon: they are listed, not in the atom order.
    let source = read("[H]C([H])([H])[H]").unwrap();
    let written = write(&graph(&source), SmilesStyle::Canonical).unwrap();
    assert_eq!(written.text, "C");
    assert_eq!(written.folded_hydrogens.len(), 4);
    assert_eq!(written.atom_order.len(), 1);
    assert!(written.folded_hydrogens.iter().all(|h| !written.atom_order.contains(h)));
    // A stated count written bare, and Kekulé bonds written aromatic, are changes.
    let stated = write(&graph(&read("[CH4]").unwrap()), SmilesStyle::Canonical).unwrap();
    assert!(stated.changes.iter().any(|c| matches!(c, WriteChange::StatedHydrogensInferred(_))), "{:?}", stated.changes);
    let kekule = write(&graph(&read("C1=CC=CC=C1").unwrap()), SmilesStyle::Canonical).unwrap();
    assert_eq!(kekule.text, "c1ccccc1");
    assert!(kekule.changes.iter().any(|c| matches!(c, WriteChange::KekuleAsAromatic(bonds) if bonds.len() == 6)), "{:?}", kekule.changes);
    // An inferred-hydrogen atom that needs brackets is a change too.
    let ammonium = write(&graph(&read("[NH4+]").unwrap()), SmilesStyle::Canonical).unwrap();
    assert_eq!(ammonium.text, "[NH4+]");
    // Aromatic input has nothing to rewrite.
    let aromatic = write(&graph(&read("c1ccccc1").unwrap()), SmilesStyle::Canonical).unwrap();
    assert!(!aromatic.changes.iter().any(|c| matches!(c, WriteChange::KekuleAsAromatic(_))));
}
