//! CIP labels of stored tetrahedral centers: the label states, the sign, the rules, the work
//! limits and determinism. See also `crates/domain/tests/cip-fixtures.json`, which
//! `scripts/verify-cip-rdkit.py` reads with RDKit alone.
//!
//! A label is a function of the stored statement and the graph only. Layout, rotation and
//! reflection of a drawing are therefore not inputs to it; there is no such test because there
//! is nothing to vary. Mirror images are tested by reversing every stored turn.

mod reference;
mod rule3;
mod support;

use std::cell::Cell;
use std::collections::BTreeMap;

use chemifree_domain::budget::{CancelFlag, Interrupt, Interrupted, NEVER};
use chemifree_domain::cip::{
    CenterReport, Descriptor, Limits, Reason, Stop, Warning, label_centers,
};
use chemifree_domain::document::{AtomId, AtomRef, BondOrder, Command};
use chemifree_domain::states::{AtomKind, AttachmentPoints, Hydrogens, QueryAtom, Radical};
use chemifree_domain::stereo::{AtomConfiguration, Relation, Slot, SlotRef, Turn};

use reference::{Reference, geometry_descriptor};
use support::{Built, Molecule, Neighbor, element, parse};

fn text(report: &CenterReport) -> String {
    use chemifree_domain::cip::State;
    match report.state {
        State::Assigned(descriptor) => descriptor.as_str().to_owned(),
        State::None => "none".to_owned(),
        State::Unspecified => "unspecified".to_owned(),
        State::Unknown => "unknown".to_owned(),
        State::Ambiguous(reason) => format!("ambiguous:{}", reason.code()),
        State::Unsupported(reason) => format!("unsupported:{}", reason.code()),
    }
}

fn run(built: &Built, atoms: &[usize], limits: Limits) -> Result<Vec<CenterReport>, Stop> {
    let ids: Vec<AtomId> = atoms.iter().map(|&i| built.ids[i]).collect();
    label_centers(&built.graph(), &ids, limits, &NEVER)
}

fn one(molecule: &Molecule, atom: usize) -> CenterReport {
    let built = molecule.build();
    run(&built, &[atom], Limits::default())
        .expect("within limits")
        .remove(0)
}

fn label(smiles: &str, atom: usize) -> String {
    text(&one(&parse(smiles), atom))
}

fn flipped_text(label: &str) -> String {
    match label {
        "R" => "S".to_owned(),
        "S" => "R".to_owned(),
        other => other.to_owned(),
    }
}

/// Seeded generator for permutations and random molecules; no id or hash order is involved.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % bound as u64).unwrap()
    }

    fn shuffled(&mut self, count: usize) -> Vec<usize> {
        let mut order: Vec<usize> = (0..count).collect();
        for i in (1..count).rev() {
            order.swap(i, self.below(i + 1));
        }
        order
    }
}

// ---------------------------------------------------------------------------------------
// The sign.
// ---------------------------------------------------------------------------------------

/// The label of a center with F, Cl, Br and I, in every listed order and both turns, equals the
/// descriptor a hand 3D construction gives. The oracle is vectors and a triple product; it
/// does not use the permutation parity the implementation uses.
#[test]
fn sign_matches_the_hand_3d_oracle_for_every_order_and_turn() {
    let permutations = [
        [0, 1, 2, 3],
        [0, 1, 3, 2],
        [0, 2, 1, 3],
        [0, 2, 3, 1],
        [0, 3, 1, 2],
        [0, 3, 2, 1],
        [1, 0, 2, 3],
        [1, 0, 3, 2],
        [1, 2, 0, 3],
        [1, 2, 3, 0],
        [1, 3, 0, 2],
        [1, 3, 2, 0],
        [2, 0, 1, 3],
        [2, 0, 3, 1],
        [2, 1, 0, 3],
        [2, 1, 3, 0],
        [2, 3, 0, 1],
        [2, 3, 1, 0],
        [3, 0, 1, 2],
        [3, 0, 2, 1],
        [3, 1, 0, 2],
        [3, 1, 2, 0],
        [3, 2, 0, 1],
        [3, 2, 1, 0],
    ];
    // Atoms 1 to 4 are F, Cl, Br, I; their CIP ranks (0 highest) are 3, 2, 1, 0.
    let rank_of_atom = [0, 3, 2, 1, 0];
    let mut cases = 0;
    for permutation in permutations {
        for turn in [Turn::Anticlockwise, Turn::Clockwise] {
            let mut molecule = Molecule::default();
            for symbol in ["C", "F", "Cl", "Br", "I"] {
                molecule.atoms.push(element(symbol));
            }
            for leaf in 1..=4 {
                molecule.bonds.push((0, leaf, BondOrder::Single));
            }
            let listed = permutation.map(|p| Neighbor::Atom(p + 1));
            molecule.centers.push(support::Center {
                atom: 0,
                neighbors: listed,
                turn,
            });
            let ranks = permutation.map(|p| rank_of_atom[p + 1]);
            let expected = geometry_descriptor(turn, ranks).to_string();
            assert_eq!(
                text(&one(&molecule, 0)),
                expected,
                "order {permutation:?} turn {turn:?}"
            );
            cases += 1;
        }
    }
    assert_eq!(cases, 48);
}

// ---------------------------------------------------------------------------------------
// Fixtures: public names, public fixtures, hand-derived cases; mirror images.
// ---------------------------------------------------------------------------------------

fn fixtures() -> Vec<serde_json::Value> {
    let file = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/cip-fixtures.json"
    ))
    .expect("fixture file");
    let document: serde_json::Value = serde_json::from_str(&file).expect("valid JSON");
    document["fixtures"].as_array().expect("fixtures").clone()
}

#[test]
fn fixtures_have_their_expected_labels_and_mirror_images_flip_them() {
    let mut checked = 0;
    for fixture in fixtures() {
        let name = fixture["name"].as_str().unwrap();
        let molecule = parse(fixture["smiles"].as_str().unwrap());
        let mirror = molecule.mirrored();
        for expectation in fixture["expect"].as_array().unwrap() {
            let expected = expectation["label"].as_str().unwrap();
            if let Some(bond) = expectation.get("bond") {
                // A double bond: its label is the same in the mirror image.
                let ends = bond.as_array().unwrap();
                let end = |i: usize| usize::try_from(ends[i].as_u64().unwrap()).unwrap();
                for image in [&molecule, &mirror] {
                    let report = rule3::bond_report(image, end(0), end(1));
                    assert_eq!(
                        rule3::state_text(report.state),
                        expected,
                        "{name} bond {bond}"
                    );
                }
                checked += 1;
                continue;
            }
            let atom = usize::try_from(expectation["atom"].as_u64().unwrap()).unwrap();
            assert_eq!(text(&one(&molecule, atom)), expected, "{name} atom {atom}");
            assert_eq!(
                text(&one(&mirror, atom)),
                flipped_text(expected),
                "{name} atom {atom}, mirror image"
            );
            checked += 1;
        }
    }
    assert!(checked >= 70, "only {checked} expectations");
}

#[test]
fn a_report_carries_the_priority_and_the_rule_that_decided() {
    // L-alanine: N > COOH > CH3 > H, all four separated by Rule 1a.
    let molecule = parse("N[C@@H](C)C(=O)O");
    let built = molecule.build();
    let report = run(&built, &[1], Limits::default()).unwrap().remove(0);
    let n = Slot::Atom(built.ids[0]);
    let methyl = Slot::Atom(built.ids[2]);
    let carboxyl = Slot::Atom(built.ids[3]);
    assert_eq!(
        report.priority,
        Some(vec![
            (n, 1),
            (carboxyl, 2),
            (methyl, 3),
            (Slot::ImplicitHydrogen, 4)
        ])
    );
    assert_eq!(report.decided_by.map(|rule| rule.as_str()), Some("1a"));
    assert!(report.warnings.is_empty());

    // Deuterium against hydrogen is separated by Rule 2 alone.
    let report = one(&parse("[C@H]([2H])(F)Cl"), 0);
    assert_eq!(report.decided_by.map(|rule| rule.as_str()), Some("2"));
}

#[test]
fn explicit_and_implicit_hydrogens_give_the_same_label() {
    let implicit = label("N[C@@H](C)C(=O)O", 1);
    assert_eq!(implicit, "S");
    // The hydrogen is drawn: the same neighbor order, then a different one.
    assert_eq!(label("N[C@@]([H])(C)C(=O)O", 1), implicit);
    assert_eq!(label("[H][C@](N)(C)C(=O)O", 1), implicit);
    assert_eq!(label("N[C@]([H])(C)C(=O)O", 1), "R");
}

#[test]
fn rule_2_separates_isotopes_and_rule_1a_over_the_whole_digraph_comes_first() {
    assert_eq!(label("Cl[C@H]([13CH3])C", 1), "S");
    assert_eq!(label("Cl[C@H](C)C", 1), "none");
    // Mass numbers outrank nothing before atomic numbers deeper in the digraph.
    assert_eq!(label("Cl[C@H]([13CH2]C)CCO", 1), "R");
}

#[test]
fn an_unlabeled_atom_against_a_label_equal_to_its_table_mass_is_refused_not_tied() {
    // Whether an unlabeled carbon is 12C or the atomic weight is not decided here: the two
    // readings tie `[12CH3]` with `CH3` or rank it below, so neither answer is given.
    for smiles in [
        "Cl[C@H]([12CH3])C",
        // Two such pairs, one of them elsewhere in the digraph.
        "Cl[C@H](C[12CH3])CC",
        // The ambiguity is met at the second sphere, after Rule 1a and 1b tied.
        "Cl[C@H]([12CH2]C)CC",
    ] {
        assert_eq!(label(smiles, 1), "unsupported:cip_mass_number", "{smiles}");
    }
    // Two labeled atoms, or two unlabeled ones, tie as before.
    assert_eq!(label("Cl[C@H]([12CH3])[12CH3]", 1), "none");
    assert_eq!(label("Cl[C@H](C)C", 1), "none");
    // Labels strictly below or above the table mass rank the same under both readings.
    assert_eq!(label("Cl[C@H]([11CH3])C", 1), "R");
    assert_eq!(label("Cl[C@H]([13CH3])C", 1), "S");
    assert_eq!(label("Cl[C@H]([11CH3])[13CH3]", 1), "R");
    // A drawn hydrogen labeled 1 is the table mass of hydrogen: the same refusal.
    assert_eq!(label("[C@H]([1H])(F)Cl", 0), "unsupported:cip_mass_number");
    // A deuterium differs from it and ranks.
    assert_eq!(label("[C@H]([2H])(F)Cl", 0), "S");
}

#[test]
fn a_tie_is_none_and_a_stored_record_on_it_is_flagged() {
    let stored = one(&parse("C[C@H](C)O"), 1);
    assert_eq!(text(&stored), "none");
    assert_eq!(stored.warnings, vec![Warning::StoredNotStereogenic]);
    // The same atom with nothing stored: not stereogenic, nothing to warn about.
    let unstored = one(&parse("CC(C)O"), 1);
    assert_eq!(text(&unstored), "none");
    assert!(unstored.warnings.is_empty());
    assert_eq!(unstored.stored, None);
}

#[test]
fn an_unstored_stereocenter_is_unspecified_and_an_unknown_record_is_unknown() {
    let unstored = one(&parse("NC(C)C(=O)O"), 1);
    assert_eq!(text(&unstored), "unspecified");
    assert!(unstored.priority.is_some());

    let molecule = parse("N[C@@H](C)C(=O)O");
    let mut built = molecule.build();
    let neighbors = molecule.centers[0]
        .neighbors
        .map(|neighbor| match neighbor {
            Neighbor::Atom(index) => SlotRef::Atom(AtomRef::Id(built.ids[index])),
            Neighbor::Hydrogen => SlotRef::ImplicitHydrogen,
            Neighbor::LonePair => SlotRef::LonePair,
        });
    built
        .document
        .apply(&[Command::SetAtomConfiguration {
            atom: AtomRef::Id(built.ids[1]),
            configuration: Some(AtomConfiguration::Unknown { neighbors }),
        }])
        .unwrap();
    let report = run(&built, &[1], Limits::default()).unwrap().remove(0);
    assert_eq!(text(&report), "unknown");
    assert!(report.priority.is_some());
}

#[test]
fn meso_and_chiral_tartaric_acid_have_the_published_descriptors() {
    let rr = parse("O[C@H]([C@@H](O)C(=O)O)C(=O)O");
    let meso = parse("O[C@H]([C@H](O)C(=O)O)C(=O)O");
    assert_eq!(
        (text(&one(&rr, 1)), text(&one(&rr, 2))),
        ("R".into(), "R".into())
    );
    assert_eq!(
        (text(&one(&meso, 1)), text(&one(&meso, 2))),
        ("R".into(), "S".into())
    );
}

#[test]
fn three_coordinate_sulfur_and_phosphorus_use_the_lone_pair_as_the_lowest_position() {
    let sulfoxide = one(&parse("C[S@](=O)CC"), 1);
    assert_eq!(text(&sulfoxide), "S");
    let last = sulfoxide.priority.unwrap().last().copied().unwrap();
    assert_eq!(last, (Slot::LonePair, 4));
    assert_eq!(label("C[P@](CC)C(C)C", 1), "R");
    // A hydrogen on the center outranks the lone pair: the phantom has atomic number 0, not 1.
    let secondary = one(&parse("[H][P@](C)CC"), 1);
    assert!(
        matches!(text(&secondary).as_str(), "R" | "S"),
        "{secondary:?}"
    );
    let ranked = secondary.priority.unwrap();
    assert_eq!(ranked[3].0, Slot::LonePair, "{ranked:?}");
}

#[test]
fn a_stored_slot_the_valence_model_would_not_name_is_used_and_flagged() {
    // A pyramidal nitrogen whose stored fourth position is a hydrogen, not the lone pair.
    let mut molecule = parse("C[N@](CC)C(C)C");
    molecule.centers[0].neighbors[1] = Neighbor::Hydrogen;
    let report = one(&molecule, 1);
    assert_eq!(text(&report), "R");
    assert_eq!(report.warnings, vec![Warning::SlotUnusual]);
    let last = report.priority.unwrap().last().copied().unwrap();
    assert_eq!(last, (Slot::ImplicitHydrogen, 4));
}

// ---------------------------------------------------------------------------------------
// Refusals: never `none`, never a guess.
// ---------------------------------------------------------------------------------------

#[test]
fn ties_that_rules_4_and_5_might_break_are_refused_not_none() {
    for smiles in [
        "C[C@H]1CC[C@@H](C)CC1",
        "C[C@H]1CC[C@H](C)CC1",
        "Cl[C@H](C=C=CC)C=C=CC",
    ] {
        assert_eq!(
            label(smiles, 1),
            "unsupported:cip_rule_4_or_5_needed",
            "{smiles}"
        );
    }
    assert_eq!(
        label("[C@H]12CCCC[C@@H]1CCCC2", 0),
        "unsupported:cip_rule_4_or_5_needed"
    );
    assert_eq!(
        label("OC(=O)[C@H](O)[C@H](O)[C@H](O)C(=O)O", 5),
        "unsupported:cip_rule_4_or_5_needed"
    );
    // The same acid's outer centers are decided at Rule 1a and labeled.
    assert_eq!(label("OC(=O)[C@H](O)[C@H](O)[C@H](O)C(=O)O", 3), "R");
    // Two propenyl ligands whose geometry is not stated could be a Z and an E one: ambiguous.
    assert_eq!(
        label("Cl[C@H](C=CC)C=CC", 1),
        "ambiguous:cip_ligand_stereo_unspecified"
    );
    assert_eq!(label("Cl[C@H](C=C)C=C", 1), "none");
}

#[test]
fn aromatic_ligands_are_read_as_mancude_or_refused() {
    let aromatic = one(&parse("C[C@H](O)c1ccccc1"), 1);
    let kekule = one(&parse("C[C@H](O)C1=CC=CC=C1"), 1);
    assert_eq!(text(&aromatic), "S");
    assert_eq!(text(&kekule), "S");
    assert_eq!(aromatic.priority.as_ref().map(Vec::len), Some(4));
    assert_eq!(
        aromatic.nodes, kekule.nodes,
        "one digraph for both spellings"
    );
    assert_eq!(label("C[C@H](O)c1ccccc1", 1), "S");
    assert_eq!(
        label("C[C@H](O)c1ccccn1", 1),
        "unsupported:cip_aromatic_system"
    );
    assert_eq!(
        label("O[C@H](c1ccccc1)c1ccccc1", 1),
        "unsupported:cip_mancude_rule_1b"
    );
}

#[test]
fn structures_the_model_cannot_expand_are_refused_with_their_reason() {
    // Two CH2 arms tie at the first sphere, so the comparison must expand their atoms.
    let base = parse("Cl[C@H](CC)CC");
    let arm = |edit: &dyn Fn(&mut Molecule)| {
        let mut molecule = base.clone();
        edit(&mut molecule);
        text(&one(&molecule, 1))
    };
    assert_eq!(arm(&|_| {}), "none");
    assert_eq!(
        arm(&|m| m.atoms[2].attachment_points = AttachmentPoints::new(&[1]).unwrap()),
        "unsupported:cip_attachment_point"
    );
    assert_eq!(
        arm(&|m| m.atoms[2].hydrogens = Hydrogens::Unknown),
        "unsupported:cip_hydrogens_unknown"
    );
    assert_eq!(
        arm(&|m| m.atoms[2].radical = Radical::Unknown),
        "unsupported:cip_hydrogens_unknown"
    );
    assert_eq!(
        arm(&|m| m.atoms[2].kind = AtomKind::Query(QueryAtom::Any)),
        "unsupported:cip_query_atom"
    );
    assert_eq!(
        arm(&|m| m.bonds[2].2 = BondOrder::Dative),
        "unsupported:cip_bond_kind"
    );
    assert_eq!(
        arm(&|m| m.bonds[2].2 = BondOrder::Unknown),
        "unsupported:cip_bond_kind"
    );
    // A metal ligand ranks by atomic number without being expanded; only its expansion fails.
    assert_eq!(
        label("Cl[C@H](C[Fe])C[Fe]", 1),
        "unsupported:cip_hydrogens_unknown"
    );
}

#[test]
fn a_center_that_fails_the_structure_test_is_refused() {
    let base = parse("N[C@@H](C)C(=O)O");
    let with = |edit: &dyn Fn(&mut Molecule)| {
        let mut molecule = base.clone();
        edit(&mut molecule);
        text(&one(&molecule, 1))
    };
    assert_eq!(
        with(&|m| m.atoms[1].radical = Radical::Doublet),
        "unsupported:cip_hydrogens_unknown"
    );
    assert_eq!(
        with(&|m| m.atoms[1].hydrogens = Hydrogens::Unknown),
        "unsupported:cip_hydrogens_unknown"
    );
    assert_eq!(
        with(&|m| m.atoms[1].attachment_points = AttachmentPoints::new(&[2]).unwrap()),
        "unsupported:cip_attachment_point"
    );
    // A stated hydrogen count the valence model contradicts is not a fifth position.
    assert_eq!(
        with(&|m| m.atoms[1].hydrogens = Hydrogens::Specified(2)),
        "unsupported:cip_center_structure"
    );
    // An unlisted element: no hydrogen model to say what the fourth position is.
    assert_eq!(
        with(&|m| m.atoms[1] = element("Fe")),
        "unsupported:cip_center_structure"
    );
    assert_eq!(
        with(&|m| m.bonds[1].2 = BondOrder::Dative),
        "unsupported:cip_bond_kind"
    );
    assert_eq!(
        with(&|m| m.bonds[1].2 = BondOrder::Aromatic),
        "unsupported:cip_center_structure"
    );
}

#[test]
fn a_named_atom_that_cannot_be_a_center_is_not_a_candidate() {
    let built = parse("CCO").build();
    let report = run(&built, &[0, 2], Limits::default()).unwrap();
    assert!(
        report
            .iter()
            .all(|r| text(r) == "unsupported:cip_not_a_candidate")
    );
    let absent = label_centers(
        &built.graph(),
        &[AtomId::from_serial(999).unwrap()],
        Limits::default(),
        &NEVER,
    )
    .unwrap();
    assert_eq!(text(&absent[0]), "unsupported:cip_not_a_candidate");
}

// ---------------------------------------------------------------------------------------
// Work limits and interrupts.
// ---------------------------------------------------------------------------------------

#[test]
fn reaching_a_node_cap_is_unsupported_never_none_and_never_a_label() {
    let tiny = Limits {
        item_nodes: 3,
        request_nodes: usize::MAX,
    };
    for (smiles, atom, uncapped) in [
        ("C[C@H](C)O", 1, "none"),
        ("O[C@H]1CCCCC1", 1, "none"),
        ("O[C@H]([C@H](O)C(=O)O)C(=O)O", 1, "R"),
        ("Cl[C@H]([13CH2]C)CCO", 1, "R"),
    ] {
        let built = parse(smiles).build();
        let capped = run(&built, &[atom], tiny).unwrap().remove(0);
        assert_eq!(text(&capped), "unsupported:cip_digraph_limit", "{smiles}");
        assert!(capped.priority.is_none() && capped.decided_by.is_none());
        assert!(
            capped.nodes > tiny.item_nodes,
            "a limit hit is charged past the cap: {smiles}"
        );
        let free = run(&built, &[atom], Limits::default()).unwrap().remove(0);
        assert_eq!(text(&free), uncapped, "{smiles} without the cap");
    }
}

/// Atom 1 has two equal branches. Each is a carbon that holds a methyl and two quaternary
/// carbons, and each of those holds a methyl and two propenyl groups. Whether two positions of
/// a quaternary carbon could differ needs the descriptors of the propenyl bonds below it, and
/// the carbon above it asks again about the same bonds, so the cost of each descriptor is
/// charged once for the whole classification, not once per question.
const TWIN_BRANCHES: &str =
    "F[C@H](C(C(/C=C/C)(/C=C/C)C)(C(/C=C/C)(/C=C/C)C)C)C(C(/C=C/C)(/C=C/C)C)(C(/C=C/C)(/C=C/C)C)C";

#[test]
fn a_descriptor_needed_by_several_questions_is_charged_once_per_classification() {
    let built = parse(TWIN_BRANCHES).build();
    let at = |item_nodes| Limits {
        item_nodes,
        request_nodes: usize::MAX,
    };
    // With each descriptor charged once the classification costs 592 nodes; charged again for
    // every question that asks for it, 624, which a cap of 600 would refuse as a limit
    // although the work needs less.
    let free = run(&built, &[1], at(usize::MAX)).unwrap().remove(0);
    assert_eq!((text(&free), free.nodes), ("none".to_owned(), 592));
    let enough = run(&built, &[1], at(600)).unwrap().remove(0);
    assert_eq!((text(&enough), enough.nodes), ("none".to_owned(), 592));
    let exact = run(&built, &[1], at(592)).unwrap().remove(0);
    assert_eq!(text(&exact), "none");
    // A cap below the real cost is still a limit, never a label.
    let short = run(&built, &[1], at(591)).unwrap().remove(0);
    assert_eq!(text(&short), "unsupported:cip_digraph_limit");
    // The same work however the molecule is numbered.
    let mut rng = Rng(0x7b57_0002_c0ff_ee11);
    let molecule = parse(TWIN_BRANCHES);
    for _ in 0..4 {
        let atom_order = rng.shuffled(molecule.atoms.len());
        let bond_order = rng.shuffled(molecule.bonds.len());
        let shuffled = molecule.build_ordered(&atom_order, &bond_order);
        let report = run(&shuffled, &[1], at(600)).unwrap().remove(0);
        assert_eq!(
            (text(&report), report.nodes),
            ("none".to_owned(), 592),
            "{atom_order:?} {bond_order:?}"
        );
    }
}

#[test]
fn the_request_cap_stops_the_request_and_does_not_depend_on_the_order() {
    let built = parse("O[C@H]([C@H](O)C(=O)O)C(=O)O").build();
    let one_item = run(&built, &[1], Limits::default()).unwrap()[0].nodes;
    assert!(one_item > 0);
    let limits = Limits {
        item_nodes: usize::MAX,
        request_nodes: one_item * 2 - 1,
    };
    for order in [[1, 2], [2, 1]] {
        assert_eq!(
            run(&built, &order, limits).err(),
            Some(Stop::RequestLimit {
                limit: limits.request_nodes
            }),
            "order {order:?}"
        );
    }
    let enough = Limits {
        request_nodes: one_item * 2,
        ..limits
    };
    // Each center needs the same nodes here (the two ends of meso-tartaric acid mirror each
    // other), so two centers fit exactly and the sum is the same in either order.
    assert!(run(&built, &[1, 2], enough).is_ok());
}

struct Counting(Cell<usize>);

impl Interrupt for Counting {
    fn check(&self) -> Result<(), Interrupted> {
        self.0.set(self.0.get() + 1);
        Ok(())
    }
}

#[test]
fn the_interrupt_is_asked_at_sphere_boundaries_and_a_cancel_yields_no_reports() {
    let built = parse("N[C@@H](C)C(=O)O").build();
    let graph = built.graph();
    let atoms = [built.ids[1]];
    let counting = Counting(Cell::new(0));
    let reports = label_centers(&graph, &atoms, Limits::default(), &counting).unwrap();
    assert_eq!(text(&reports[0]), "S");
    assert!(counting.0.get() > 0, "the interrupt was never asked");
    let plain = label_centers(&graph, &atoms, Limits::default(), &NEVER).unwrap();
    assert_eq!(
        plain, reports,
        "an interrupt that never fires changes nothing"
    );

    let cancel = CancelFlag::new();
    cancel.cancel();
    assert_eq!(
        label_centers(&graph, &atoms, Limits::default(), &cancel).err(),
        Some(Stop::Interrupted(Interrupted::Cancelled))
    );
}

/// Always stops, as a deadline that has passed would.
struct Expired;

impl Interrupt for Expired {
    fn check(&self) -> Result<(), Interrupted> {
        Err(Interrupted::TimeLimit)
    }
}

#[test]
fn an_interrupt_is_seen_even_when_the_first_comparison_already_decides() {
    // F, Cl, Br and I differ in atomic number at the first position of the first sphere, so no
    // comparison goes beyond it; the request must still stop.
    let built = parse("F[C@](Cl)(Br)I").build();
    let graph = built.graph();
    let atoms = [built.ids[1]];
    assert_eq!(
        text(&label_centers(&graph, &atoms, Limits::default(), &NEVER).unwrap()[0]),
        "S"
    );
    let cancel = CancelFlag::new();
    cancel.cancel();
    assert_eq!(
        label_centers(&graph, &atoms, Limits::default(), &cancel).err(),
        Some(Stop::Interrupted(Interrupted::Cancelled))
    );
    assert_eq!(
        label_centers(&graph, &atoms, Limits::default(), &Expired).err(),
        Some(Stop::Interrupted(Interrupted::TimeLimit))
    );
}

// ---------------------------------------------------------------------------------------
// Determinism: renumbering, insertion order, work counts.
// ---------------------------------------------------------------------------------------

/// What a report says in molecule terms, with atom ids mapped back to atom indices.
fn normalized(built: &Built, report: &CenterReport) -> String {
    let index = |slot: Slot| match slot {
        Slot::Atom(id) => format!("atom{}", built.ids.iter().position(|&i| i == id).unwrap()),
        Slot::ImplicitHydrogen => "H".to_owned(),
        Slot::LonePair => "LP".to_owned(),
    };
    let priority: Vec<String> = report
        .priority
        .iter()
        .flatten()
        .map(|&(slot, rank)| format!("{}={rank}", index(slot)))
        .collect();
    format!(
        "{} {:?} {:?} warnings={:?} nodes={}",
        text(report),
        priority,
        report.decided_by,
        report.warnings,
        report.nodes
    )
}

#[test]
fn renumbering_atoms_and_reordering_insertion_changes_nothing_not_even_the_work() {
    let mut rng = Rng(0x5eed_1234_abcd_9876);
    let capped = Limits {
        item_nodes: 40,
        request_nodes: usize::MAX,
    };
    let mut compared = 0;
    for fixture in fixtures() {
        let molecule = parse(fixture["smiles"].as_str().unwrap());
        let atoms: Vec<usize> = fixture["expect"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|e| e["atom"].as_u64())
            .map(|atom| usize::try_from(atom).unwrap())
            .collect();
        for limits in [Limits::default(), capped] {
            let base = molecule.build();
            let expected: Vec<String> = run(&base, &atoms, limits)
                .unwrap()
                .iter()
                .map(|r| normalized(&base, r))
                .collect();
            for _ in 0..6 {
                let atom_order = rng.shuffled(molecule.atoms.len());
                let bond_order = rng.shuffled(molecule.bonds.len());
                let built = molecule.build_ordered(&atom_order, &bond_order);
                let got: Vec<String> = run(&built, &atoms, limits)
                    .unwrap()
                    .iter()
                    .map(|r| normalized(&built, r))
                    .collect();
                assert_eq!(
                    got, expected,
                    "{} {atom_order:?} {bond_order:?}",
                    fixture["name"]
                );
                compared += 1;
            }
        }
    }
    assert!(compared > 300);
}

/// `count` centers, each with two equal chains of `length` carbons that end in an iron atom,
/// which the valence model cannot expand: the chains tie sphere by sphere to the end, and the
/// comparison is refused there. Returns the molecule and the index of each center.
fn refused_after_long_chains(count: usize, length: usize) -> (Molecule, Vec<usize>) {
    let chain = "C".repeat(length);
    let one = parse(&format!("Cl[C@H]({chain}[Fe]){chain}[Fe]"));
    let mut molecule = Molecule::default();
    let centers = (0..count)
        .map(|_| molecule.append(&one) + 1)
        .collect::<Vec<_>>();
    (molecule, centers)
}

#[test]
fn many_centers_refused_after_a_large_expansion_still_hit_the_request_cap() {
    let (molecule, centers) = refused_after_long_chains(4, 200);
    let built = molecule.build();
    let alone = run(&built, &centers[..1], Limits::default())
        .unwrap()
        .remove(0);
    assert_eq!(text(&alone), "unsupported:cip_hydrogens_unknown");
    // The refused comparison expanded both chains (about three nodes per carbon: the next
    // carbon and two hydrogens), and the report says so.
    assert!(alone.nodes > 1000, "nodes {}", alone.nodes);

    let limits = Limits {
        item_nodes: usize::MAX,
        request_nodes: alone.nodes * 3,
    };
    for order in [[0, 1, 2, 3], [3, 2, 1, 0]] {
        let atoms: Vec<usize> = order.iter().map(|&i| centers[i]).collect();
        assert_eq!(
            run(&built, &atoms, limits).err(),
            Some(Stop::RequestLimit {
                limit: limits.request_nodes
            }),
            "four refused items of {} nodes each, order {order:?}",
            alone.nodes
        );
    }
    let enough = Limits {
        request_nodes: alone.nodes * 4,
        ..limits
    };
    let reports = run(&built, &centers, enough).unwrap();
    assert!(reports.iter().all(|r| r.nodes == alone.nodes));
}

/// A spine of `levels` carbons that each carry a methyl, as a SMILES fragment.
fn spine(levels: usize) -> String {
    format!("{}C", "C(C)".repeat(levels))
}

/// The label of a center with a fluorine, a chlorine, a methyl and a spine, computed on a
/// thread with the 2 MiB stack Rust gives a spawned thread (and tokio gives a worker).
fn spine_label(levels: usize, limits: Limits) -> CenterReport {
    let smiles = format!("[C@](F)(Cl)(C){}", spine(levels));
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || {
            let built = parse(&smiles).build();
            run(&built, &[0], limits).unwrap().remove(0)
        })
        .unwrap()
        .join()
        .expect("the labeler overflowed its stack")
}

#[test]
fn a_digraph_deeper_than_the_sphere_bound_is_refused_and_does_not_overflow_the_stack() {
    // Every carbon of the spine ties with its methyl at the root atom, so ordering its
    // children compares the rest of the spine, which orders the children of the next carbon:
    // one level of recursion per sphere. 900 levels are 1,801 atoms, within a document.
    let levels = 900;
    let report = spine_label(levels, Limits::default());
    assert_eq!(text(&report), "unsupported:cip_digraph_limit");
    assert!(report.priority.is_none() && report.decided_by.is_none());
    // The same bound holds without a node cap that would stop it first.
    let uncapped = Limits {
        item_nodes: usize::MAX,
        request_nodes: usize::MAX,
    };
    assert_eq!(
        text(&spine_label(levels, uncapped)),
        "unsupported:cip_digraph_limit"
    );
    // A spine just within the bound is ranked (on the same 2 MiB stack): Rule 1a separates
    // it from the methyl. One level more reaches the sphere the bound refuses.
    for levels in [200, 254] {
        let short = spine_label(levels, uncapped);
        assert!(
            matches!(text(&short).as_str(), "R" | "S"),
            "{levels} levels: {}",
            text(&short)
        );
    }
    assert_eq!(
        text(&spine_label(256, uncapped)),
        "unsupported:cip_digraph_limit"
    );
}

/// Molecules whose comparisons are refused, with the centers to label. Several refusals of
/// different kinds sit at the same depth in different branches, so which one a lazy expansion
/// meets first would follow the identifiers if the code let it.
fn refused_molecules() -> Vec<(&'static str, Molecule, Vec<usize>)> {
    let mut cases = Vec::new();
    let arm = "CC(CCCC)C";
    let two_metals = parse(&format!("Cl[C@H]({arm}[Fe]){arm}[Fe]"));
    cases.push(("two metals", two_metals.clone(), vec![1]));

    let mut query_and_metal = parse(&format!("Cl[C@H]({arm}[Fe]){arm}C"));
    let last = query_and_metal.atoms.len() - 1;
    query_and_metal.atoms[last].kind = AtomKind::Query(QueryAtom::Any);
    cases.push(("query atom and metal", query_and_metal, vec![1]));

    let mut dative = parse(&format!("Cl[C@H]({arm}[Fe]){arm}C"));
    let last_bond = dative.bonds.len() - 1;
    dative.bonds[last_bond].2 = BondOrder::Dative;
    cases.push(("dative bond and metal", dative, vec![1]));

    let mut attached = parse(&format!("Cl[C@H]({arm}C){arm}C"));
    attached.atoms[3].attachment_points = AttachmentPoints::new(&[1]).unwrap();
    attached.atoms[12].hydrogens = Hydrogens::Unknown;
    cases.push(("attachment point and unknown hydrogens", attached, vec![1]));

    let mass = parse(&format!("Cl[C@H]({arm}[12CH3]){arm}C"));
    cases.push(("unlabeled against its table mass", mass, vec![1]));

    let pyridine = parse("C[C@H](O)c1ccccn1");
    cases.push(("aromatic system", pyridine, vec![1]));
    let diphenyl = parse("O[C@H](c1ccccc1)c1ccccc1");
    cases.push(("mancude rule 1b", diphenyl, vec![1]));
    cases
}

/// A generated molecule with some atoms the model cannot expand or compare.
fn random_refused_molecule(rng: &mut Rng) -> Molecule {
    let mut molecule = random_molecule(rng);
    for _ in 0..1 + rng.below(3) {
        let index = rng.below(molecule.atoms.len());
        match rng.below(3) {
            0 => molecule.atoms[index] = element("Fe"),
            1 => {
                molecule.atoms[index].isotope = None;
                molecule.atoms[index].kind = AtomKind::Query(QueryAtom::Any);
            }
            _ => {
                if !molecule.bonds.is_empty() {
                    let bond = rng.below(molecule.bonds.len());
                    molecule.bonds[bond].2 = BondOrder::Dative;
                }
            }
        }
    }
    // A stored statement about a bond that is no longer a double bond cannot be kept.
    let bonds = molecule.bonds.clone();
    molecule
        .doubles
        .retain(|double| bonds[double.bond].2 == BondOrder::Double);
    // A center must stay an ordinary carbon with ordinary bonds to be given a configuration.
    let intact = |molecule: &Molecule, atom: usize| {
        molecule.atoms[atom].kind.element().map(|e| e.symbol()) == Some("C")
            && molecule
                .bonds
                .iter()
                .filter(|&&(a, b, _)| a == atom || b == atom)
                .all(|(_, _, order)| *order != BondOrder::Dative)
    };
    let kept: Vec<_> = molecule
        .centers
        .iter()
        .filter(|center| intact(&molecule, center.atom))
        .cloned()
        .collect();
    molecule.centers = kept;
    molecule
}

#[test]
fn refused_items_report_the_same_reason_and_work_whatever_the_identifiers() {
    let mut rng = Rng(0x0bad_cafe_1357_9bdf);
    let mut cases = refused_molecules();
    for _ in 0..300 {
        let molecule = random_refused_molecule(&mut rng);
        let centers: Vec<usize> = molecule.centers.iter().map(|c| c.atom).collect();
        cases.push(("generated", molecule, centers));
    }
    let tight = Limits {
        item_nodes: 60,
        request_nodes: usize::MAX,
    };
    let (mut refused, mut compared) = (0, 0);
    for (name, molecule, centers) in &cases {
        for limits in [Limits::default(), tight] {
            let base = molecule.build();
            let expected: Vec<String> = run(&base, centers, limits)
                .unwrap()
                .iter()
                .map(|r| normalized(&base, r))
                .collect();
            refused += expected
                .iter()
                .filter(|e| e.starts_with("unsupported:") && !e.contains("nodes=0"))
                .count();
            for _ in 0..8 {
                let atom_order = rng.shuffled(molecule.atoms.len());
                let bond_order = rng.shuffled(molecule.bonds.len());
                let built = molecule.build_ordered(&atom_order, &bond_order);
                let got: Vec<String> = run(&built, centers, limits)
                    .unwrap()
                    .iter()
                    .map(|r| normalized(&built, r))
                    .collect();
                assert_eq!(got, expected, "{name} {atom_order:?} {bond_order:?}");
                compared += 1;
            }
        }
    }
    assert!(refused > 100, "only {refused} refused items with work");
    assert!(compared > 4000);
}

// ---------------------------------------------------------------------------------------
// A naive reference implementation on generated molecules.
// ---------------------------------------------------------------------------------------

fn valence_of(symbol: &str) -> usize {
    match symbol {
        "C" => 4,
        "N" => 3,
        "O" => 2,
        _ => 1,
    }
}

/// A random molecule of up to 12 atoms: a tree, sometimes one ring closure, with double bonds,
/// isotopic carbons and drawn isotopic hydrogens, and a random stored turn on every center.
fn random_molecule(rng: &mut Rng) -> Molecule {
    let symbols = ["C", "C", "C", "C", "C", "N", "O", "F", "Cl"];
    let mut molecule = Molecule::default();
    let mut used: Vec<usize> = Vec::new();
    fn push(molecule: &mut Molecule, used: &mut Vec<usize>, symbol: &str, heavy: bool) -> usize {
        let mut atom = element(symbol);
        if symbol == "C" && heavy {
            atom.isotope = Some(13);
        }
        molecule.atoms.push(atom);
        used.push(0);
        molecule.atoms.len() - 1
    }
    push(&mut molecule, &mut used, "C", false);
    let size = 5 + rng.below(8);
    // A molecule whose atoms are all saturated stops growing early.
    for _ in 0..200 {
        if molecule.atoms.len() >= size {
            break;
        }
        let symbol = symbols[rng.below(symbols.len())];
        let parent = rng.below(molecule.atoms.len());
        let parent_symbol = molecule.atoms[parent].kind.element().unwrap().symbol();
        let order = if rng.below(6) == 0 && symbol != "F" && symbol != "Cl" {
            2
        } else {
            1
        };
        let free = |used: &Vec<usize>, i: usize, symbol: &str| valence_of(symbol) - used[i];
        if free(&used, parent, parent_symbol) < order || valence_of(symbol) < order {
            continue;
        }
        // No cumulated double bonds: an allene end is a refusal of its own (tested elsewhere).
        let has_double = |molecule: &Molecule, atom: usize| {
            molecule
                .bonds
                .iter()
                .any(|&(a, b, ref o)| (a == atom || b == atom) && *o == BondOrder::Double)
        };
        if order == 2 && has_double(&molecule, parent) {
            continue;
        }
        let heavy = rng.below(5) == 0;
        let child = push(&mut molecule, &mut used, symbol, heavy);
        used[parent] += order;
        used[child] += order;
        molecule.bonds.push((
            parent,
            child,
            if order == 2 {
                BondOrder::Double
            } else {
                BondOrder::Single
            },
        ));
    }
    if rng.below(3) == 0 {
        let (a, b) = (
            rng.below(molecule.atoms.len()),
            rng.below(molecule.atoms.len()),
        );
        let adjacent = molecule
            .bonds
            .iter()
            .any(|&(x, y, _)| (x, y) == (a, b) || (x, y) == (b, a));
        let spare = |i: usize, molecule: &Molecule, used: &[usize]| {
            valence_of(molecule.atoms[i].kind.element().unwrap().symbol()) > used[i]
        };
        if a != b && !adjacent && spare(a, &molecule, &used) && spare(b, &molecule, &used) {
            used[a] += 1;
            used[b] += 1;
            molecule.bonds.push((a, b, BondOrder::Single));
        }
    }
    // Drawn isotopic hydrogens on some atoms with a free valence.
    for (i, &already) in used.clone().iter().enumerate() {
        let symbol = molecule.atoms[i].kind.element().unwrap().symbol();
        if already < valence_of(symbol) && rng.below(4) == 0 {
            let mut hydrogen = element("H");
            hydrogen.isotope = Some(if rng.below(2) == 0 { 2 } else { 3 });
            molecule.atoms.push(hydrogen);
            used.push(1);
            used[i] += 1;
            let h = molecule.atoms.len() - 1;
            molecule.bonds.push((i, h, BondOrder::Single));
        }
    }
    // Stored statements about some double bonds: relative (either reference position at each
    // end, so the flip rule is exercised), stated unknown, or none.
    for index in 0..molecule.bonds.len() {
        let (a, b, ref order) = molecule.bonds[index];
        if *order != BondOrder::Double {
            continue;
        }
        let kind = rng.below(8);
        if kind >= 5 {
            continue;
        }
        let mut position = |atom: usize, partner: usize| {
            let symbol = molecule.atoms[atom].kind.element().unwrap().symbol();
            let mut list: Vec<Neighbor> = molecule
                .bonds
                .iter()
                .filter_map(|&(x, y, _)| (x == atom).then_some(y).or((y == atom).then_some(x)))
                .filter(|&other| other != partner)
                .map(Neighbor::Atom)
                .collect();
            for _ in 0..valence_of(symbol) - used[atom] {
                list.push(Neighbor::Hydrogen);
            }
            while list.len() < 2 {
                list.push(Neighbor::LonePair);
            }
            list.truncate(2);
            list[rng.below(2)]
        };
        let (begin, end) = (position(a, b), position(b, a));
        molecule.doubles.push(support::Double {
            bond: index,
            begin,
            end,
            relation: match kind {
                0 | 1 => Some(Relation::SameSide),
                2 | 3 => Some(Relation::OppositeSides),
                _ => None,
            },
        });
    }
    for (i, &spent) in used.iter().enumerate() {
        let symbol = molecule.atoms[i].kind.element().unwrap().symbol();
        let neighbors: Vec<usize> = molecule
            .bonds
            .iter()
            .filter_map(|&(a, b, _)| (a == i).then_some(b).or((b == i).then_some(a)))
            .collect();
        let all_single = molecule
            .bonds
            .iter()
            .filter(|&&(a, b, _)| a == i || b == i)
            .all(|(_, _, order)| *order == BondOrder::Single);
        let implicit = valence_of(symbol) - spent;
        if symbol == "C" && all_single && neighbors.len() >= 3 && neighbors.len() + implicit == 4 {
            let mut listed: Vec<Neighbor> = neighbors.iter().map(|&n| Neighbor::Atom(n)).collect();
            if implicit == 1 {
                listed.push(Neighbor::Hydrogen);
            }
            let order = rng.shuffled(4);
            let neighbors: [Neighbor; 4] = [
                listed[order[0]],
                listed[order[1]],
                listed[order[2]],
                listed[order[3]],
            ];
            let turn = if rng.below(2) == 0 {
                Turn::Anticlockwise
            } else {
                Turn::Clockwise
            };
            molecule.centers.push(support::Center {
                atom: i,
                neighbors,
                turn,
            });
        }
    }
    molecule
}

/// A center with a chlorine, a hydrogen and two copies of one random fragment, so that its two
/// ligands tie until the stored geometry of their double bonds (chosen independently for each
/// copy: Z, E, unknown or none) is read. Atom 0 is the center.
fn random_twin_molecule(rng: &mut Rng) -> Molecule {
    let mut base = random_molecule(rng);
    base.centers.clear();
    let free = |molecule: &Molecule, atom: usize| {
        let used: usize = molecule
            .bonds
            .iter()
            .filter(|&&(a, b, _)| a == atom || b == atom)
            .map(|(_, _, order)| if *order == BondOrder::Double { 2 } else { 1 })
            .sum();
        valence_of(molecule.atoms[atom].kind.element().unwrap().symbol()) > used
    };
    let open: Vec<usize> = (0..base.atoms.len())
        .filter(|&atom| {
            free(&base, atom) && base.atoms[atom].kind.element().unwrap().symbol() == "C"
        })
        .collect();
    if open.is_empty() {
        return base;
    }
    let attach = open[rng.below(open.len())];
    // A statement about a bond at the attaching atom would name positions that change.
    let bonds = base.bonds.clone();
    base.doubles.retain(|double| {
        let (a, b, _) = bonds[double.bond];
        a != attach && b != attach
    });
    let mut second = base.clone();
    for double in &mut second.doubles {
        double.relation = match rng.below(4) {
            0 => Some(Relation::SameSide),
            1 => Some(Relation::OppositeSides),
            2 => None,
            _ => double.relation,
        };
    }
    let mut molecule = Molecule::default();
    molecule.atoms.push(element("C"));
    molecule.atoms.push(element("Cl"));
    molecule.bonds.push((0, 1, BondOrder::Single));
    let (first, other) = (molecule.append(&base), molecule.append(&second));
    molecule.bonds.push((0, first + attach, BondOrder::Single));
    molecule.bonds.push((0, other + attach, BondOrder::Single));
    let order = rng.shuffled(4);
    let listed = [
        Neighbor::Atom(1),
        Neighbor::Hydrogen,
        Neighbor::Atom(first + attach),
        Neighbor::Atom(other + attach),
    ];
    molecule.centers.push(support::Center {
        atom: 0,
        neighbors: [
            listed[order[0]],
            listed[order[1]],
            listed[order[2]],
            listed[order[3]],
        ],
        turn: if rng.below(2) == 0 {
            Turn::Anticlockwise
        } else {
            Turn::Clockwise
        },
    });
    molecule
}

/// Alternates the plain generator with the twin one, so ties between ligands with double bonds
/// are common.
fn generated(rng: &mut Rng, index: usize) -> Molecule {
    if index % 3 == 2 {
        random_twin_molecule(rng)
    } else {
        random_molecule(rng)
    }
}

#[test]
fn the_implementation_agrees_with_a_naive_reference_on_generated_molecules() {
    let mut rng = Rng(0x1234_5678_9abc_def1);
    let (mut assigned, mut tied_none, mut refused, mut ambiguous, mut centers, mut cycles) =
        (0, 0, 0, 0, 0, 0);
    let mut nested = 0;
    for index in 0..4500 {
        let molecule = generated(&mut rng, index);
        let built = molecule.build();
        let acyclic = molecule.bonds.len() + 1 == molecule.atoms.len();
        for center in &molecule.centers {
            centers += 1;
            let reference = Reference::new(&molecule, center.atom);
            let report = run(&built, &[center.atom], Limits::default())
                .unwrap()
                .remove(0);
            let symbols: Vec<String> = molecule
                .atoms
                .iter()
                .map(|a| {
                    format!(
                        "{}{}",
                        a.kind.element().unwrap().symbol(),
                        a.isotope.map_or(String::new(), |i| i.to_string())
                    )
                })
                .collect();
            let shown = format!(
                "{symbols:?} {:?} {:?} {:?}",
                molecule.bonds, center, molecule.doubles
            );
            if !acyclic && text(&report) == "unsupported:cip_aromatic_system" {
                // A ring with a double bond that the aromaticity model cannot classify is
                // refused rather than read as Kekule; the naive reference does not refuse.
                refused += 1;
                continue;
            }
            if text(&report) == "unsupported:cip_rule3_nested_descriptor" {
                // A descriptor read through the labeled center's own ring or double bond
                // (see `Reason::Rule3NestedDescriptor`): the naive reference reads it from the
                // whole molecule and does not refuse. Only a molecule with a double bond can
                // reach this.
                assert!(
                    molecule
                        .bonds
                        .iter()
                        .any(|bond| bond.2 == BondOrder::Double),
                    "{shown}"
                );
                nested += 1;
                continue;
            }
            let found = reference.ranks(center.neighbors);
            if found.cycle {
                // Not comparable: see `Found::cycle`.
                cycles += 1;
                continue;
            }
            let shown_text = text(&report);
            // Where an unstated double-bond geometry (or a descriptor that needs itself) took
            // part, the implementation may answer ambiguous or refuse; where it answers a label
            // it must be the reference's. Everywhere else the two agree exactly.
            // A tie at an end of a ligand double bond may be refused for Rules 4 and 5.
            let lenient_ok = (found.lenient
                && (shown_text.starts_with("ambiguous:")
                    || shown_text.starts_with("unsupported:")))
                || (found.end_tie && shown_text == "unsupported:cip_rule_4_or_5_needed");
            match found.ranks {
                Some(ranks) => {
                    let expected = geometry_descriptor(center.turn, ranks).to_string();
                    if lenient_ok {
                        ambiguous += usize::from(shown_text.starts_with("ambiguous:"));
                    } else {
                        assert_eq!(shown_text, expected, "{shown}");
                        assigned += 1;
                    }
                }
                None => match shown_text.as_str() {
                    "none" => {
                        if acyclic {
                            for (i, j) in reference.equal_pairs(center.neighbors) {
                                assert_eq!(
                                    reference.canonical_ligand(center.neighbors[i]),
                                    reference.canonical_ligand(center.neighbors[j]),
                                    "none on ligands that are not equivalent: {shown}"
                                );
                            }
                        }
                        tied_none += 1;
                    }
                    "unsupported:cip_rule_4_or_5_needed" => refused += 1,
                    _ if lenient_ok => ambiguous += 1,
                    other => panic!("a tie became {other}: {shown}"),
                },
            }
        }
    }
    assert!(centers > 2500, "only {centers} centers");
    assert!(
        assigned > 300 && tied_none > 50 && refused > 10 && ambiguous > 5,
        "{assigned} {tied_none} {refused} {ambiguous} {cycles}"
    );
    assert!(nested < centers / 20, "{nested} of {centers} refused");
    assert!(
        cycles * 20 < centers,
        "{cycles} of {centers} not comparable"
    );
}

#[test]
fn the_mirror_image_of_every_generated_center_flips_its_label() {
    let mut rng = Rng(0xfeed_beef_0001_0203);
    let mut flipped = 0;
    for _ in 0..800 {
        let molecule = random_molecule(&mut rng);
        let mirror = molecule.mirrored();
        for center in &molecule.centers {
            let a = text(&one(&molecule, center.atom));
            let b = text(&one(&mirror, center.atom));
            assert_eq!(b, flipped_text(&a));
            flipped += usize::from(a == "R" || a == "S");
        }
    }
    assert!(flipped > 100);
}

#[test]
fn descriptors_have_four_values_and_reasons_distinct_codes() {
    // The vocabulary: R and S for centers, E and Z for double bonds; r, s are reserved.
    let names: Vec<_> = [Descriptor::R, Descriptor::S, Descriptor::E, Descriptor::Z]
        .iter()
        .map(|d| d.as_str())
        .collect();
    assert_eq!(names, ["R", "S", "E", "Z"]);
    let codes: BTreeMap<_, _> = [
        Reason::DoubleBondStructure,
        Reason::StereoCycle,
        Reason::Rule3NestedDescriptor,
        Reason::LigandStereoUnspecified,
        Reason::DigraphLimit,
    ]
    .iter()
    .map(|r| (r.code(), ()))
    .collect();
    assert_eq!(codes.len(), 5);
    assert_eq!(
        Reason::Rule3NestedDescriptor.code(),
        "cip_rule3_nested_descriptor"
    );
}
