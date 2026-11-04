//! Rule 3 (Z before E) and the E/Z labels of double bonds.
//!
//! The expected labels of the public molecules were also read from RDKit 2022.03.2
//! (`scripts/verify-cip-rdkit.py` over `cip-fixtures.json`); the molecules that differ from
//! RDKit are marked in their tests.

use std::collections::BTreeSet;

use chemifree_domain::budget::{CancelFlag, NEVER};
use chemifree_domain::cip::{
    BondReport, Candidates, Limits, Reason, State, Stop, Warning, candidates, label_centers,
    label_double_bonds,
};
use chemifree_domain::document::{BondId, BondOrder};
use chemifree_domain::stereo::Relation;

use crate::reference::Reference;
use crate::support::{Built, Double, Molecule, Neighbor, parse};
use crate::{Rng, text};

pub fn state_text(state: State) -> String {
    match state {
        State::Assigned(descriptor) => descriptor.as_str().to_owned(),
        State::None => "none".to_owned(),
        State::Unspecified => "unspecified".to_owned(),
        State::Unknown => "unknown".to_owned(),
        State::Ambiguous(reason) => format!("ambiguous:{}", reason.code()),
        State::Unsupported(reason) => format!("unsupported:{}", reason.code()),
    }
}

/// The index of the bond between atoms `a` and `b` of the molecule.
fn bond_index(molecule: &Molecule, a: usize, b: usize) -> usize {
    molecule
        .bonds
        .iter()
        .position(|&(x, y, _)| (x, y) == (a, b) || (x, y) == (b, a))
        .unwrap_or_else(|| panic!("no bond {a}-{b}"))
}

fn bond_reports(built: &Built, bonds: &[usize], limits: Limits) -> Result<Vec<BondReport>, Stop> {
    let ids: Vec<BondId> = bonds.iter().map(|&i| built.bond_ids[i]).collect();
    label_double_bonds(&built.graph(), &ids, limits, &NEVER)
}

pub fn bond_report(molecule: &Molecule, a: usize, b: usize) -> BondReport {
    let built = molecule.build();
    let index = bond_index(molecule, a, b);
    bond_reports(&built, &[index], Limits::default())
        .expect("within limits")
        .remove(0)
}

fn bond_label(smiles: &str, a: usize, b: usize) -> String {
    state_text(bond_report(&parse(smiles), a, b).state)
}

fn center_label(smiles: &str, atom: usize) -> String {
    text(&crate::one(&parse(smiles), atom))
}

/// The molecule of `smiles` with its double bond `a`=`b` stated by hand: `begin` and `end` name
/// positions of the bond's begin and end atom (as written in the SMILES) and `relation` their
/// relation, or `None` for stated unknown.
fn with_record(
    smiles: &str,
    (a, b): (usize, usize),
    begin: Neighbor,
    end: Neighbor,
    relation: Option<Relation>,
) -> Molecule {
    let mut molecule = parse(smiles);
    let bond = bond_index(&molecule, a, b);
    molecule.doubles.retain(|double| double.bond != bond);
    let (first, _, _) = molecule.bonds[bond];
    let (begin, end) = if first == a {
        (begin, end)
    } else {
        (end, begin)
    };
    molecule.doubles.push(Double {
        bond,
        begin,
        end,
        relation,
    });
    molecule
}

// ---------------------------------------------------------------------------------------
// E and Z of a double bond.
// ---------------------------------------------------------------------------------------

#[test]
fn e_and_z_follow_the_higher_substituents_on_the_same_or_opposite_sides() {
    // (atom a, atom b of the double bond, label); every label is also RDKit's.
    for (smiles, a, b, expected) in [
        ("F/C=C/Cl", 1, 2, "E"),
        ("F/C=C\\Cl", 1, 2, "Z"),
        ("Cl/C=C/Cl", 1, 2, "E"),
        ("Cl/C=C\\Cl", 1, 2, "Z"),
        ("C/C=C/C", 1, 2, "E"),
        ("C/C=C\\C", 1, 2, "Z"),
        ("OC(=O)/C=C/C(=O)O", 3, 4, "E"),
        ("OC(=O)/C=C\\C(=O)O", 3, 4, "Z"),
        // The substituents' atom numbers are in the other order.
        ("Cl/C=C/F", 1, 2, "E"),
        ("Cl/C=C\\F", 1, 2, "Z"),
        // The higher substituent is the second one written at an end.
        ("C(/Cl)(F)=C/Br", 0, 3, "Z"),
        ("C(\\Cl)(F)=C/Br", 0, 3, "E"),
    ] {
        assert_eq!(bond_label(smiles, a, b), expected, "{smiles}");
    }
}

#[test]
fn a_double_bond_without_a_record_is_unspecified_and_a_stated_unknown_is_unknown() {
    assert_eq!(bond_label("FC=CCl", 1, 2), "unspecified");
    let unknown = with_record("FC=CCl", (1, 2), Neighbor::Atom(0), Neighbor::Atom(3), None);
    assert_eq!(state_text(bond_report(&unknown, 1, 2).state), "unknown");
    // The report names the higher substituent at each end.
    let report = bond_report(&unknown, 1, 2);
    let ends = report.ends.expect("two distinct ends");
    assert_eq!(ends[0].decided_by.as_str(), "1a");
}

#[test]
fn an_end_with_two_equal_substituents_is_not_stereogenic_and_a_record_there_is_flagged() {
    assert_eq!(bond_label("C=C", 0, 1), "none");
    assert_eq!(bond_label("ClC(Cl)=CF", 1, 3), "none");
    // Two methyl groups tie; so do two CH2CH3 groups, at the whole digraph.
    assert_eq!(bond_label("CC(C)=CCl", 1, 3), "none");
    let stored = with_record(
        "FC(F)=CCl",
        (1, 3),
        Neighbor::Atom(0),
        Neighbor::Atom(4),
        Some(Relation::SameSide),
    );
    let report = bond_report(&stored, 1, 3);
    assert_eq!(state_text(report.state), "none");
    assert_eq!(report.warnings, [Warning::StoredNotStereogenic]);
    assert!(report.ends.is_none());
    // A carbonyl oxygen has two lone pairs: a stored record on C=O is flagged too.
    let carbonyl = with_record(
        "CC(=O)C",
        (1, 2),
        Neighbor::Atom(0),
        Neighbor::LonePair,
        Some(Relation::SameSide),
    );
    let report = bond_report(&carbonyl, 1, 2);
    assert_eq!(state_text(report.state), "none");
    assert_eq!(report.warnings, [Warning::StoredNotStereogenic]);
}

#[test]
fn the_stored_reference_may_name_either_position_of_either_end() {
    // (E)-F-CH=CH-Cl: F and Cl on opposite sides. Naming the hydrogen at an end instead of the
    // heavy neighbor flips the relation, once per end that is renamed.
    let h = Neighbor::Hydrogen;
    let (f, cl) = (Neighbor::Atom(0), Neighbor::Atom(3));
    let opposite = Some(Relation::OppositeSides);
    let same = Some(Relation::SameSide);
    for (begin, end, relation, expected) in [
        (f, cl, opposite, "E"),
        (f, cl, same, "Z"),
        (h, cl, same, "E"),
        (h, cl, opposite, "Z"),
        (f, h, same, "E"),
        (f, h, opposite, "Z"),
        (h, h, opposite, "E"),
        (h, h, same, "Z"),
    ] {
        let molecule = with_record("FC=CCl", (1, 2), begin, end, relation);
        assert_eq!(
            state_text(bond_report(&molecule, 1, 2).state),
            expected,
            "{begin:?} {end:?} {relation:?}"
        );
    }
    // The same statements with the bond written the other way round, so that its begin and
    // end atoms swap: "ClC=CF" has the bond 1=2 with begin Cl-side.
    for (begin, end, relation, expected) in [
        (cl_side(), f_side(), opposite, "E"),
        (cl_side(), f_side(), same, "Z"),
        (h, f_side(), same, "E"),
        (cl_side(), h, same, "E"),
    ] {
        let molecule = with_record("ClC=CF", (1, 2), begin, end, relation);
        assert_eq!(
            state_text(bond_report(&molecule, 1, 2).state),
            expected,
            "reversed {begin:?} {end:?} {relation:?}"
        );
    }
}

fn cl_side() -> Neighbor {
    Neighbor::Atom(0)
}

fn f_side() -> Neighbor {
    Neighbor::Atom(3)
}

#[test]
fn imines_use_the_lone_pair_as_the_lowest_position() {
    // C=N: the nitrogen has the lone pair and one substituent (or a hydrogen).
    assert_eq!(bond_label("C/C=N/O", 1, 2), "E");
    assert_eq!(bond_label("C/C=N\\O", 1, 2), "Z");
    // A hydrogen outranks the lone pair: N-H imine.
    assert_eq!(bond_label("F/C=N/[H]", 1, 2), "E");
}

#[test]
fn isotopes_alone_decide_the_ends_of_a_double_bond() {
    // 13CH3 outranks CH3 by Rule 2 at the first end; the other end is decided by Rule 1a.
    for (smiles, a, b, expected) in [
        ("[13CH3]/C(C)=C/C", 1, 3, "E"),
        ("[13CH3]/C(C)=C\\C", 1, 3, "Z"),
        // The same molecules with the atoms written in the reverse order.
        ("C/C=C(\\C)[13CH3]", 1, 2, "E"),
        ("C/C=C(/C)[13CH3]", 1, 2, "Z"),
        // Deuterium against hydrogen.
        ("[2H]/C=C/C", 1, 2, "E"),
        ("C/C=C/[2H]", 1, 2, "E"),
        ("[2H]/C=C\\C", 1, 2, "Z"),
    ] {
        assert_eq!(bond_label(smiles, a, b), expected, "{smiles}");
    }
    // Without the isotope the same record is on a tie (and flagged).
    let plain = with_record(
        "CC(C)=CC",
        (1, 3),
        Neighbor::Atom(0),
        Neighbor::Atom(4),
        Some(Relation::SameSide),
    );
    assert_eq!(state_text(bond_report(&plain, 1, 3).state), "none");
}

#[test]
fn rule_1a_over_the_whole_digraph_decides_before_rule_2_at_a_double_bond() {
    // At the first end: CH2CH2Cl against 13CH2CH3. Rule 2 alone would rank the 13C branch
    // higher at sphere 1, but Rule 1a finds Cl at sphere 3 first, so CH2CH2Cl is higher.
    // RDKit agrees on both molecules.
    assert_eq!(bond_label("C/C=C(\\CCCl)[13CH2]C", 1, 2), "Z");
    assert_eq!(bond_label("C/C=C(/CCCl)[13CH2]C", 1, 2), "E");
    // The same bond written from the other end.
    assert_eq!(bond_label("C[13CH2]/C(CCCl)=C/C", 2, 6), "Z");
}

// ---------------------------------------------------------------------------------------
// Rule 3 in the comparison of ligands.
// ---------------------------------------------------------------------------------------

#[test]
fn a_z_alkenyl_ligand_outranks_an_e_one() {
    // Both ligands are propenyl; only Rule 3 separates them. Z is the higher one, so listing
    // the Z ligand first (with F lowest-but-one) gives S, and the swapped molecule R. RDKit
    // agrees on every line.
    assert_eq!(center_label("F[C@H](/C=C\\C)/C=C/C", 1), "S");
    assert_eq!(center_label("F[C@H](/C=C/C)/C=C\\C", 1), "R");
    // The double bonds one sphere deeper.
    assert_eq!(center_label("F[C@H](C/C=C\\C)C/C=C/C", 1), "S");
    assert_eq!(center_label("F[C@H](C/C=C/C)C/C=C\\C", 1), "R");
    // N-hydroxy imines: the nitrogen end has a lone pair.
    assert_eq!(center_label("F[C@H](/C=N/O)/C=N\\O", 1), "R");
    // Two Z ligands tie.
    assert_eq!(center_label("F[C@H](/C=C\\C)/C=C\\C", 1), "none");
    let report = crate::one(&parse("F[C@H](/C=C\\C)/C=C/C"), 1);
    assert_eq!(report.decided_by.map(|rule| rule.as_str()), Some("3"));
}

#[test]
fn rule_3_comes_after_rule_2() {
    // The E ligand carries 13C, which Rule 2 ranks first; Rule 3 would rank the Z ligand first.
    // The two molecules differ by the turn only, so a Rule 3 decision would give the same label
    // for both. Rule 2 decides the opposite way.
    let rule_2 = center_label("F[C@H](/C=C\\C)/C=C/[13CH3]", 1);
    let rule_3 = center_label("F[C@H](/C=C\\C)/C=C/C", 1);
    assert_eq!(rule_3, "S");
    assert_eq!(rule_2, "R");
}

#[test]
fn a_ligand_double_bond_with_no_stated_geometry_is_ambiguous_not_a_guess() {
    // RDKit ranks an unspecified double bond below E and labels these S and R. The rules this
    // model rests on do not say where an unspecified bond goes, and it could be a Z, so the
    // answer is ambiguous, and so is an unspecified bond against an unspecified one.
    for (smiles, expected) in [
        (
            "F[C@H](/C=C\\C)C=CC",
            "ambiguous:cip_ligand_stereo_unspecified",
        ),
        (
            "F[C@H](C=CC)/C=C\\C",
            "ambiguous:cip_ligand_stereo_unspecified",
        ),
        (
            "F[C@H](/C=C/C)C=CC",
            "ambiguous:cip_ligand_stereo_unspecified",
        ),
        (
            "F[C@H](C=CC)C=CC",
            "ambiguous:cip_ligand_stereo_unspecified",
        ),
    ] {
        assert_eq!(center_label(smiles, 1), expected, "{smiles}");
    }
    // A stated unknown is no better than no statement.
    let unknown = with_record(
        "F[C@H](/C=C\\C)/C=C/C",
        (5, 6),
        Neighbor::Atom(1),
        Neighbor::Atom(7),
        None,
    );
    assert_eq!(
        text(&crate::one(&unknown, 1)),
        "ambiguous:cip_ligand_stereo_unspecified"
    );
    let report = crate::one(&unknown, 1);
    assert!(report.priority.is_none() && report.decided_by.is_none());
    // An unspecified bond that the ranking never reaches does not matter: Rule 1a decides.
    assert_eq!(center_label("Cl[C@H](C=CC)C(F)(F)F", 1), "R");
    // ... and where it is compared only with something it cannot equal, nothing is ambiguous:
    // CH=CH-CH3 against a ligand that has no double bond at that place is decided earlier.
    assert_eq!(center_label("F[C@H](CC)C=CC", 1), "R");
}

#[test]
fn an_unspecified_ligand_double_bond_beside_a_plain_one_is_not_ambiguous() {
    // CH=C(CH3)2 (not stereogenic) against CH=CH-CH3 differ at Rule 1a; the pair that does
    // reach Rule 3 here is two vinyl groups, whose bond is not stereogenic: a tie.
    assert_eq!(center_label("Cl[C@H](C=C)C=C", 1), "none");
}

#[test]
fn a_tie_that_contains_an_unspecified_double_bond_is_ambiguous_not_none() {
    // Two identical propenyl ligands might differ (Z and E): the center may be stereogenic.
    assert_eq!(
        center_label("Cl[C@H](C=CC)C=CC", 1),
        "ambiguous:cip_ligand_stereo_unspecified"
    );
    // The same ligands with the geometry stated equal are a real tie.
    assert_eq!(center_label("Cl[C@H](/C=C/C)/C=C/C", 1), "none");
}

// ---------------------------------------------------------------------------------------
// Renumbering, mirror images and the ends of a double bond with Rule 3 inside.
// ---------------------------------------------------------------------------------------

#[test]
fn the_ends_of_a_double_bond_can_be_separated_by_rule_3() {
    // The central bond is atoms 1=2 (F-CH=C). Its second end has a Z-propenyl and an E-propenyl
    // substituent that only Rule 3 separates; Z is the higher one. RDKit gives E and Z here.
    let molecule = parse("F/C=C(/C=C\\C)\\C=C\\C");
    let report = bond_report(&molecule, 1, 2);
    assert_eq!(state_text(report.state), "E");
    let ends = report.ends.expect("distinct ends");
    assert_eq!(ends[0].decided_by.as_str(), "1a");
    assert_eq!(ends[1].decided_by.as_str(), "3");
    // The Z propenyl on the other side of the bond gives the other label.
    assert_eq!(bond_label("F/C=C(\\C=C/C)/C=C/C", 1, 2), "Z");
    // With one of the propenyl groups unspecified the end cannot be ranked (RDKit ranks it
    // below Z and says E; this model does not).
    assert_eq!(
        bond_label("F/C=C(/C=C\\C)C=CC", 1, 2),
        "ambiguous:cip_ligand_stereo_unspecified"
    );
}

/// A molecule with the atoms to label as centers and the double bonds to label, by atom index.
type Labeled = (&'static str, Vec<usize>, Vec<(usize, usize)>);

/// Molecules that exercise the new states.
fn rule_3_molecules() -> Vec<Labeled> {
    vec![
        ("F/C=C/Cl", vec![], vec![(1, 2)]),
        (
            "F/C=C(/C=C\\C)\\C=C\\C",
            vec![],
            vec![(1, 2), (3, 4), (6, 7)],
        ),
        ("F[C@H](/C=C\\C)/C=C/C", vec![1], vec![(2, 3), (5, 6)]),
        ("F[C@H](/C=C\\C)C=CC", vec![1], vec![(2, 3), (5, 6)]),
        ("Cl[C@H](C=CC)C=CC", vec![1], vec![(2, 3), (5, 6)]),
        ("F[C@H](C/C=C\\C)C/C=C/C", vec![1], vec![(3, 4), (7, 8)]),
        ("[13CH3]/C(C)=C/C", vec![], vec![(1, 3)]),
        ("C/C=C(\\CCCl)[13CH2]C", vec![], vec![(1, 2)]),
        ("FC=CCl", vec![], vec![(1, 2)]),
        ("C[C@H](/C=C/C)/C=C/C(F)F", vec![1], vec![(2, 3), (5, 6)]),
        ("CC(C)=CF", vec![], vec![(1, 3)]),
        ("C/C=N/O", vec![], vec![(1, 2)]),
    ]
}

/// The state of every named unit as text, each unit named by the atom indices of the molecule so
/// that a renumbered build compares equal.
fn describe(
    molecule: &Molecule,
    built: &Built,
    centers: &[usize],
    bonds: &[(usize, usize)],
    limits: Limits,
) -> Vec<String> {
    let mut lines = Vec::new();
    let ids: Vec<_> = centers.iter().map(|&i| built.ids[i]).collect();
    let name = |id| built.ids.iter().position(|&x| x == id).unwrap();
    for report in label_centers(&built.graph(), &ids, limits, &NEVER).unwrap() {
        lines.push(format!(
            "atom {} {} {:?} {:?}",
            name(report.atom),
            state_text(report.state),
            report.priority.as_ref().map(|p| p.len()),
            report.decided_by.map(|r| r.as_str()),
        ));
        lines.push(format!("nodes {}", report.nodes));
    }
    for &(a, b) in bonds {
        let index = bond_index(molecule, a, b);
        let report = bond_reports(built, &[index], limits).unwrap().remove(0);
        let higher = report.ends.map(|ends| {
            ends.map(|end| {
                let position = match end.higher {
                    chemifree_domain::stereo::Slot::Atom(id) => {
                        built.ids.iter().position(|&x| x == id).unwrap().to_string()
                    }
                    other => format!("{other:?}"),
                };
                format!(
                    "{}:{}:{}",
                    name(end.atom),
                    position,
                    end.decided_by.as_str()
                )
            })
        });
        lines.push(format!(
            "bond {a}-{b} {} {higher:?} {:?} nodes {}",
            state_text(report.state),
            report.warnings,
            report.nodes
        ));
    }
    lines
}

#[test]
fn renumbering_changes_no_state_reason_rule_or_work_of_a_center_or_double_bond() {
    let mut rng = Rng(0x00c0_ffee_1234_5678);
    for (smiles, centers, bonds) in rule_3_molecules() {
        let molecule = parse(smiles);
        let plain = describe(
            &molecule,
            &molecule.build(),
            &centers,
            &bonds,
            Limits::default(),
        );
        for _ in 0..40 {
            let atom_order = rng.shuffled(molecule.atoms.len());
            let bond_order = rng.shuffled(molecule.bonds.len());
            let built = molecule.build_ordered(&atom_order, &bond_order);
            // The ids differ from the indices; `describe` names units by index through `ids`.
            let permuted = describe(&molecule, &built, &centers, &bonds, Limits::default());
            assert_eq!(permuted, plain, "{smiles} {atom_order:?} {bond_order:?}");
        }
        // Under a tight cap the same holds: the work counted does not follow identifiers.
        let tight = Limits {
            item_nodes: 40,
            request_nodes: usize::MAX,
        };
        let reference = describe(&molecule, &molecule.build(), &centers, &bonds, tight);
        for _ in 0..20 {
            let atom_order = rng.shuffled(molecule.atoms.len());
            let bond_order = rng.shuffled(molecule.bonds.len());
            let built = molecule.build_ordered(&atom_order, &bond_order);
            assert_eq!(
                describe(&molecule, &built, &centers, &bonds, tight),
                reference,
                "{smiles} under a tight cap"
            );
        }
    }
}

#[test]
fn a_mirror_image_flips_every_center_and_leaves_every_double_bond_alone() {
    for (smiles, centers, bonds) in rule_3_molecules() {
        let molecule = parse(smiles);
        let mirror = molecule.mirrored();
        for (a, b) in &bonds {
            assert_eq!(
                state_text(bond_report(&molecule, *a, *b).state),
                state_text(bond_report(&mirror, *a, *b).state),
                "{smiles} bond {a}-{b}"
            );
        }
        for &center in &centers {
            let before = text(&crate::one(&molecule, center));
            let after = text(&crate::one(&mirror, center));
            assert_eq!(
                after,
                crate::flipped_text(&before),
                "{smiles} atom {center}"
            );
        }
    }
}

// ---------------------------------------------------------------------------------------
// Refusals, structure, and what a label never does.
// ---------------------------------------------------------------------------------------

#[test]
fn double_bonds_the_model_cannot_label_are_refused_with_their_reason() {
    // Not a double bond; absent.
    assert_eq!(bond_label("FCCl", 1, 2), "unsupported:cip_not_a_candidate");
    assert_eq!(
        bond_label("FC#CCl", 1, 2),
        "unsupported:cip_not_a_candidate"
    );
    // Allene: the end of a cumulated bond (ST11).
    assert_eq!(
        bond_label("FC=C=CCl", 1, 2),
        "unsupported:cip_double_bond_structure"
    );
    // A metal at an end, a dative bond, a query atom.
    assert_eq!(
        bond_label("C(F)(Cl)=[Fe]", 0, 3),
        "unsupported:cip_hydrogens_unknown"
    );
    // An aromatic or mancude bond.
    assert_eq!(
        bond_label("c1ccccc1", 0, 1),
        "unsupported:cip_aromatic_system"
    );
    assert_eq!(
        bond_label("C1=CC=CC=C1", 0, 1),
        "unsupported:cip_aromatic_system"
    );
}

#[test]
fn a_cumulated_ligand_double_bond_is_not_read_as_e_or_z_and_its_axis_is_refused_in_a_tie() {
    // A cumulated ligand double bond has no E or Z: for Rule 3 it is a bond that is not
    // stereogenic, and the axis is left to Rules 4 and 5, which refuse a tie that contains one
    // whose ends could differ. Terminal allenes tie as plain ligands.
    assert_eq!(center_label("F[C@H](C=C=C)C=C=C", 1), "none");
    assert_eq!(
        center_label("F[C@H](C=C=CC)C=C=CC", 1),
        "unsupported:cip_rule_4_or_5_needed"
    );
}

#[test]
fn labeling_never_changes_the_graph_marks_or_stored_records() {
    // ST03: the label of an E/Z bond is a separate output; it does not touch coordinates,
    // marks or the stored record. Labeling takes a graph snapshot and returns reports.
    let molecule = parse("F/C=C/Cl");
    let built = molecule.build();
    let before = built.graph();
    let ids: Vec<BondId> = built.bond_ids.clone();
    let _ = label_double_bonds(&before, &ids, Limits::default(), &NEVER).unwrap();
    let all_atoms: Vec<_> = built.ids.clone();
    let _ = label_centers(&before, &all_atoms, Limits::default(), &NEVER).unwrap();
    assert_eq!(before, built.graph());
    let document_graph = built.graph();
    assert_eq!(document_graph.stereo().len(), 1);
    assert!(!document_graph.has_stereo() || document_graph.stereo().len() == 1);
}

// ---------------------------------------------------------------------------------------
// Limits, interrupt and work of double bonds.
// ---------------------------------------------------------------------------------------

#[test]
fn a_cancelled_request_yields_no_double_bond_reports() {
    let molecule = parse("F/C=C/Cl");
    let built = molecule.build();
    let flag = CancelFlag::new();
    flag.cancel();
    let outcome = label_double_bonds(&built.graph(), &built.bond_ids, Limits::default(), &flag);
    assert!(matches!(outcome, Err(Stop::Interrupted(_))));
}

#[test]
fn the_request_cap_stops_a_request_of_double_bonds_in_either_order() {
    let molecule = parse("F/C=C(/C=C\\C)\\C=C\\C");
    let built = molecule.build();
    let bonds = [
        bond_index(&molecule, 1, 2),
        bond_index(&molecule, 3, 4),
        bond_index(&molecule, 6, 7),
    ];
    let measured = bond_reports(&built, &bonds, Limits::default()).unwrap();
    let total: usize = measured.iter().map(|report| report.nodes).sum();
    assert!(total > 0);
    for order in [[0, 1, 2], [2, 1, 0], [1, 2, 0]] {
        let listed: Vec<usize> = order.iter().map(|&i| bonds[i]).collect();
        let tight = Limits {
            item_nodes: usize::MAX / 4,
            request_nodes: total - 1,
        };
        assert!(
            matches!(
                bond_reports(&built, &listed, tight),
                Err(Stop::RequestLimit { .. })
            ),
            "{order:?}"
        );
        let enough = Limits {
            item_nodes: usize::MAX / 4,
            request_nodes: total,
        };
        assert!(bond_reports(&built, &listed, enough).is_ok(), "{order:?}");
    }
}

#[test]
fn a_cap_on_an_item_is_unsupported_never_none_and_never_a_label() {
    let molecule = parse("F/C=C(/C=C\\C)\\C=C\\C");
    let built = molecule.build();
    let index = bond_index(&molecule, 1, 2);
    for cap in [0, 1, 3, 6] {
        let tight = Limits {
            item_nodes: cap,
            request_nodes: usize::MAX,
        };
        let report = bond_reports(&built, &[index], tight).unwrap().remove(0);
        assert_eq!(
            state_text(report.state),
            "unsupported:cip_digraph_limit",
            "cap {cap}"
        );
    }
}

// ---------------------------------------------------------------------------------------
// The candidate scan.
// ---------------------------------------------------------------------------------------

fn scan(smiles: &str) -> (Molecule, Built, Candidates) {
    let molecule = parse(smiles);
    let built = molecule.build();
    let found = candidates(&built.graph(), Limits::default(), &NEVER).expect("within limits");
    (molecule, built, found)
}

#[test]
fn the_scan_lists_stereogenic_units_with_or_without_a_stored_record() {
    // 2-butanol has one center and no stored record: unspecified. Its other atoms are not units.
    let (_, built, found) = scan("CC(O)CC");
    assert_eq!(found.centers.len(), 1);
    assert_eq!(found.centers[0].atom, built.ids[1]);
    assert_eq!(state_text(found.centers[0].state), "unspecified");
    assert!(found.double_bonds.is_empty());
    // A stored configuration is listed too.
    let (_, built, found) = scan("C[C@H](O)CC");
    assert_eq!(found.centers.len(), 1);
    assert_eq!(found.centers[0].atom, built.ids[1]);
    assert!(matches!(found.centers[0].state, State::Assigned(_)));
    // Double bonds with distinct substituents at each end: unspecified without a record.
    let (_, built, found) = scan("CC=CC");
    assert_eq!(found.double_bonds.len(), 1);
    assert_eq!(found.double_bonds[0].bond, built.bond_ids[1]);
    assert_eq!(state_text(found.double_bonds[0].state), "unspecified");
    // Not stereogenic: an isopropyl center, a terminal double bond, a carbonyl, a CH2.
    let (_, _, found) = scan("CC(C)C=C");
    assert!(found.centers.is_empty() && found.double_bonds.is_empty());
    let (_, _, found) = scan("CC(=O)C");
    assert!(found.centers.is_empty() && found.double_bonds.is_empty());
}

#[test]
fn the_scan_lists_what_it_cannot_decide_and_never_a_ring_double_bond() {
    // Two unspecified propenyl ligands on a center: the center is listed as ambiguous, and so
    // are the two double bonds as unspecified.
    let (_, built, found) = scan("ClC(C=CC)C=CC");
    assert_eq!(found.centers.len(), 1);
    assert_eq!(
        state_text(found.centers[0].state),
        "ambiguous:cip_ligand_stereo_unspecified"
    );
    assert_eq!(found.double_bonds.len(), 2);
    assert!(
        found
            .double_bonds
            .iter()
            .all(|r| state_text(r.state) == "unspecified")
    );
    let _ = built;
    // A ring double bond is never a candidate, stored or not, but its substituents may be.
    let (_, _, found) = scan("C1CCC=CC1");
    assert!(found.double_bonds.is_empty());
    let (_, _, found) = scan("C1CCC/C=C\\CC1");
    assert!(found.double_bonds.is_empty());
    // A center that needs Rules 4 and 5 is listed as unsupported: it may be stereogenic.
    let (_, built, found) = scan("CC1CCC(C)CC1");
    let listed: BTreeSet<_> = found.centers.iter().map(|r| r.atom).collect();
    assert!(listed.contains(&built.ids[1]), "{listed:?}");
    assert!(
        found
            .centers
            .iter()
            .all(|r| matches!(r.state, State::Unsupported(Reason::Rule4Or5Needed)))
    );
}

#[test]
fn the_scan_checks_the_interrupt_even_when_it_skips_every_atom_and_bond() {
    // A long acyclic chain has no center and no double bond, so the scan examines nothing and
    // never ranks anything: only the scan's own checkpoint can stop it.
    let chain = parse(&"C".repeat(60));
    let built = chain.build();
    let flag = CancelFlag::new();
    flag.cancel();
    assert!(matches!(
        candidates(&built.graph(), Limits::default(), &flag),
        Err(Stop::Interrupted(_))
    ));
    // The same chain without a cancel has nothing to list.
    let found = candidates(&built.graph(), Limits::default(), &NEVER).unwrap();
    assert!(found.centers.is_empty() && found.double_bonds.is_empty());
}

#[test]
fn the_scan_stops_at_the_request_cap() {
    let molecule = parse("CC(O)CC=CC");
    let built = molecule.build();
    let tight = Limits {
        item_nodes: 20_000,
        request_nodes: 3,
    };
    assert!(matches!(
        candidates(&built.graph(), tight, &NEVER),
        Err(Stop::RequestLimit { .. })
    ));
}

// ---------------------------------------------------------------------------------------
// The naive reference on double bonds.
// ---------------------------------------------------------------------------------------

#[test]
fn double_bond_labels_agree_with_a_naive_side_assignment_on_generated_molecules() {
    let mut rng = Rng(0x0dd5_eed5_0011_2233);
    let (mut z, mut e, mut none, mut lenient) = (0, 0, 0, 0);
    for index in 0..3000 {
        let molecule = crate::generated(&mut rng, index);
        let built = molecule.build();
        let reference = Reference::new(&molecule, 0);
        for (index, &(a, b, ref order)) in molecule.bonds.iter().enumerate() {
            if *order != BondOrder::Double {
                continue;
            }
            let acyclic = molecule.bonds.len() + 1 == molecule.atoms.len();
            let report = bond_reports(&built, &[index], Limits::default())
                .unwrap()
                .remove(0);
            let shown = text_of(&report);
            let in_a_ring = !acyclic;
            if in_a_ring && shown == "unsupported:cip_aromatic_system" {
                continue;
            }
            match reference.bond_label(a, b) {
                Some("none") => {
                    // The implementation answers none, or refuses a tie that Rules 4 and 5
                    // might break.
                    assert!(
                        shown == "none"
                            || shown == "unsupported:cip_rule_4_or_5_needed"
                            || shown == "unsupported:cip_digraph_limit"
                            || shown == "unsupported:cip_rule3_nested_descriptor"
                            || shown == "unsupported:cip_stereo_cycle",
                        "{shown} for a tie in {:?}",
                        molecule.bonds
                    );
                    none += usize::from(shown == "none");
                }
                Some(label) if reference.is_stored(a, b) => {
                    if shown.starts_with("unsupported:") || shown.starts_with("ambiguous:") {
                        lenient += 1;
                    } else {
                        assert_eq!(shown, label, "{:?} {:?}", molecule.bonds, molecule.doubles);
                        z += usize::from(label == "Z");
                        e += usize::from(label == "E");
                    }
                }
                Some(_) => {
                    // Not stored: the implementation says unspecified (or unknown when stated).
                    assert!(
                        shown == "unspecified"
                            || shown == "unknown"
                            || shown.starts_with("unsupported:"),
                        "{shown}"
                    );
                }
                None => {
                    assert!(
                        shown == "unspecified"
                            || shown == "unknown"
                            || shown.starts_with("ambiguous:")
                            || shown.starts_with("unsupported:"),
                        "{shown}"
                    );
                    lenient += 1;
                }
            }
        }
    }
    assert!(z > 40 && e > 40 && none > 40, "{z} {e} {none} {lenient}");
}

fn text_of(report: &BondReport) -> String {
    state_text(report.state)
}

#[test]
fn double_bonds_whose_descriptors_need_each_other_are_a_stereo_cycle_not_a_guess() {
    // Tetra-ethylidenecyclobutane: at each ring carbon the two ring branches meet different
    // double bonds (the neighbors' own), so only those descriptors could separate them, and each
    // needs the first's in turn. A stated record changes nothing: the ranking is what cannot be
    // finished.
    let smiles = "CC=C1C(=CC)C(=CC)C1=CC";
    let plain = parse(smiles);
    let doubles = double_bonds_of(&plain);
    assert_eq!(doubles.len(), 4);
    let built = plain.build();
    let forward = bond_reports(&built, &doubles, Limits::default()).unwrap();
    let reversed: Vec<usize> = doubles.iter().rev().copied().collect();
    let backward = bond_reports(&built, &reversed, Limits::default()).unwrap();
    for report in forward.iter().chain(&backward) {
        assert_eq!(
            state_text(report.state),
            "unsupported:cip_stereo_cycle",
            "{report:?}"
        );
        assert!(report.ends.is_none());
    }
    // Neither the order of the request nor which bond asked first changes what a bond is charged.
    for (i, report) in forward.iter().enumerate() {
        assert_eq!(report.nodes, backward[3 - i].nodes);
    }
    assert!(forward[0].nodes > 0 && forward[0].nodes < 1000);
    let stated = with_record(
        smiles,
        (1, 2),
        Neighbor::Atom(0),
        Neighbor::Atom(3),
        Some(Relation::SameSide),
    );
    let report = bond_report(&stated, 1, 2);
    assert_eq!(state_text(report.state), "unsupported:cip_stereo_cycle");
    assert!(report.stored.is_some());

    // The same ring with nothing to separate its branches is not a cycle: Rule 3 is not asked.
    let methylene = parse("C=C1C(=C)C(=C)C1=C");
    let doubles: Vec<usize> = (0..methylene.bonds.len())
        .filter(|&i| methylene.bonds[i].2 == BondOrder::Double)
        .collect();
    for report in bond_reports(&methylene.build(), &doubles, Limits::default()).unwrap() {
        assert_eq!(state_text(report.state), "none");
    }
}

/// Every permutation of `0..count`, in a fixed order.
fn permutations(count: usize) -> Vec<Vec<usize>> {
    if count == 0 {
        return vec![Vec::new()];
    }
    let mut all = Vec::new();
    for tail in permutations(count - 1) {
        for place in 0..count {
            let mut order = tail.clone();
            order.insert(place, count - 1);
            all.push(order);
        }
    }
    all
}

/// The indices of the double bonds of a molecule.
fn double_bonds_of(molecule: &Molecule) -> Vec<usize> {
    (0..molecule.bonds.len())
        .filter(|&i| molecule.bonds[i].2 == BondOrder::Double)
        .collect()
}

/// The label, reason and charge of every double bond of the molecule, asked for in each order
/// of the bonds: the lists (by bond index) must be identical, and that list is returned.
fn same_in_every_order(molecule: &Molecule) -> Vec<(String, usize)> {
    let built = molecule.build();
    let doubles = double_bonds_of(molecule);
    let mut seen: Option<Vec<(String, usize)>> = None;
    for order in permutations(doubles.len()) {
        let asked: Vec<usize> = order.iter().map(|&i| doubles[i]).collect();
        let reports = bond_reports(&built, &asked, Limits::default()).unwrap();
        let mut by_bond = vec![(String::new(), 0); doubles.len()];
        for (&position, report) in order.iter().zip(&reports) {
            by_bond[position] = (state_text(report.state), report.nodes);
        }
        match &seen {
            None => seen = Some(by_bond),
            Some(first) => assert_eq!(first, &by_bond, "asked in the order {order:?}"),
        }
    }
    seen.unwrap()
}

#[test]
fn two_ring_branches_that_reach_the_same_double_bond_do_not_make_a_stereo_cycle() {
    // Neither molecule is stereogenic at either exocyclic bond: the two ring branches at an end
    // meet the other double bond alike (it is one bond, so its descriptor is one value), and
    // RDKit's labeler gives none for both bonds.
    for smiles in ["CC=C1CCC(=CC)CC1", "CC=C1CC(=CC)C1"] {
        for (label, _) in same_in_every_order(&parse(smiles)) {
            assert_eq!(label, "none", "{smiles}");
        }
        // The scan lists neither, and a stored record is flagged as stored on a bond that is not
        // stereogenic.
        let (_, _, found) = scan(smiles);
        assert!(found.double_bonds.is_empty(), "{smiles}");
    }
    let stated = with_record(
        "CC=C1CCC(=CC)CC1",
        (1, 2),
        Neighbor::Atom(0),
        Neighbor::Atom(3),
        Some(Relation::SameSide),
    );
    let report = bond_report(&stated, 1, 2);
    assert_eq!(state_text(report.state), "none");
    assert_eq!(report.warnings, vec![Warning::StoredNotStereogenic]);
}

#[test]
fn double_bonds_asked_in_any_order_get_the_same_label_reason_and_charge() {
    // The first two are symmetric rings where one bond's analysis meets a cycle refusal while
    // another bond is being computed. A result computed under that refusal must not be reused
    // by a bond asked for later: asked for in the order E1, M1, E2, M2 the ethylidene bonds were
    // a stereo cycle, and asked for E2 first E2 was `none`. Neither bond is stereogenic.
    let none = "none";
    let cycle = "unsupported:cip_stereo_cycle";
    for (smiles, expected) in [
        ("C(=CC)1C(=C)C(=CC)C(=C)1", vec![none; 4]),
        ("C1C(=CC)CC(=C)CC(=CC)CC(=C)1", vec![none; 4]),
        ("C(=C)1C(=C)C(=C)C(=C)1", vec![none; 4]),
        // Not a cycle of labels, but the work counted depended on which bond was asked first
        // when a result computed under a refusal was kept.
        ("C(=C)1C(C)C(=C)C(C)C(=C)C(C)1", vec![none; 3]),
        ("CC=C1CCC(=CC)CC1", vec![none; 2]),
        // Genuine cycles: the four ethylidene bonds, and the 1,3,5-trisubstituted ring whose
        // branches meet different bonds at every step.
        ("CC=C1C(=CC)C(=CC)C1=CC", vec![cycle; 4]),
        ("C1C(=CC)CC(=CC)CC1=CC", vec![cycle; 3]),
    ] {
        let labels = same_in_every_order(&parse(smiles));
        let seen: Vec<&str> = labels.iter().map(|(label, _)| label.as_str()).collect();
        assert_eq!(seen, expected, "{smiles}");
        for (bond, (_, nodes)) in labels.iter().enumerate() {
            assert!(*nodes > 0 && *nodes < 5000, "{smiles} bond {bond}: {nodes}");
        }
        // The scan asks in the order of the bond identifiers and lists what is not `none`; each
        // listed bond has the label and the charge it has when asked for alone.
        let (molecule, built, found) = scan(smiles);
        let doubles = double_bonds_of(&molecule);
        let listed: Vec<(String, usize)> = found
            .double_bonds
            .iter()
            .map(|report| (state_text(report.state), report.nodes))
            .collect();
        let alone: Vec<(String, usize)> = doubles
            .iter()
            .zip(&labels)
            .filter(|(_, (label, _))| label != "none")
            .map(|(_, entry)| entry.clone())
            .collect();
        assert_eq!(listed, alone, "{smiles}");
        let ids: Vec<BondId> = doubles.iter().map(|&i| built.bond_ids[i]).collect();
        assert!(found.double_bonds.iter().all(|r| ids.contains(&r.bond)));
    }
}

#[test]
fn a_tied_end_makes_a_bond_not_stereogenic_whatever_the_other_end_cannot_say() {
    // The bond is not stereogenic if either end has two substituents that tie, so an ambiguity
    // or a refusal at the other end does not matter and the answer is `none`. The same other end
    // beside a stereogenic end is what the ambiguity or refusal is about.
    for (other_end, reason) in [
        ("(C=CC)C=CC", "ambiguous:cip_ligand_stereo_unspecified"),
        ("(C[Fe])C[Fe]", "unsupported:cip_hydrogens_unknown"),
    ] {
        assert_eq!(bond_label(&format!("CC=C{other_end}"), 1, 2), reason);
        assert_eq!(bond_label(&format!("C=C{other_end}"), 0, 1), "none");
    }
    // The exception is a limit: an end whose ranking ran past the sphere bound is charged more
    // than the item may use, and the item is then a limit whatever the other end says.
    let arm = format!("{}[2H]", "C(C)".repeat(40) + "C");
    let plain = "C(C)".repeat(40) + "C";
    for smiles in [format!("CC=C({arm}){plain}"), format!("C=C({arm}){plain}")] {
        let at = if smiles.starts_with("CC") {
            (1, 2)
        } else {
            (0, 1)
        };
        assert_eq!(
            bond_label(&smiles, at.0, at.1),
            "unsupported:cip_digraph_limit"
        );
    }
}

#[test]
fn a_tie_that_reads_nothing_makes_the_bond_not_stereogenic_whatever_the_other_end_reads() {
    // Each arm is CH2-C(isobutyl)=CH-CH3 with an E bond. The two arms tie at their carbon by
    // Rule 3, which reads the descriptors of those inner bonds, and an inner bond separates its
    // two substituents (the CH2 back toward the carbon, the isobutyl) only at the carbon's own
    // substituents: the whole molecule has the other end of the labeled bond there and the
    // digraph of the labeled bond has not, so the inner descriptors read differently from the
    // bond. With =CH2 at the other end the bond is not stereogenic whatever they say, because
    // its two hydrogens tie without reading any descriptor. RDKit 2022.03.2 gives `none`.
    let arms = "(/C/C(CC(C)C)=C/C)\\C/C(CC(C)C)=C/C";
    assert_eq!(bond_label(&format!("C=C{arms}"), 0, 1), "none");
    // Beside an end that does not tie (CH3 and H) the tie of the arms is all that would make
    // the bond not stereogenic, and it rests on those descriptors: refused (RDKit: `none`).
    assert_eq!(
        bond_label(&format!("C/C=C{arms}"), 1, 2),
        "unsupported:cip_rule3_nested_descriptor"
    );
}

/// The nodes a center is charged when its two ligands are the same cage or fused polycycle: the
/// worst case for a tie, since both digraphs are read to their last sphere and classified. A
/// deterministic count of the accounting in the module comment of `cip.rs`; a change to the
/// accounting changes these numbers, and the caps should be looked at again with them. The
/// measured values are in the report of the change that added them (issue #113, follow-up 3).
#[test]
fn the_default_caps_hold_for_cage_and_fused_polycyclic_ligands_except_the_cubyl_pair() {
    let measured = [
        ("adamantyl", "C12CC3CC(CC(C3)C1)C2", 6696),
        ("twistyl", "C1C2CC3CC1CC23", 1674),
        ("norbornyl", "C1CC2CCC1C2", 654),
        ("bicyclo[2.2.2]octyl", "C1CC2CCC1CC2", 1446),
        ("bispidyl", "C1C2CNCC1CN2", 494),
        ("decalyl", "C1CCC2CCCCC2C1", 726),
        ("gonyl", "C1CCC2C(C1)CCC1C2CCC2CCCC12", 3258),
        ("perhydrotriphenylyl", "C1CCC2C(C1)C1CCCCC1C1CCCCC21", 3384),
        ("perhydropyrenyl", "C1CC2CCC3CCC4CCC1C2C34", 5100),
        ("cubyl", "C12C3C4C1C5C2C3C45", 119_646),
    ];
    let unlimited = Limits {
        item_nodes: 50_000_000,
        request_nodes: 500_000_000,
    };
    for (name, ligand, nodes) in measured {
        let built = parse(&format!("F[C@H](C{ligand})C{ligand}")).build();
        let report = crate::run(&built, &[1], unlimited)
            .expect("within the raised limits")
            .remove(0);
        assert_eq!(report.nodes, nodes, "{name}");
        assert_eq!(
            text(&report),
            "unsupported:cip_rule_4_or_5_needed",
            "{name}: two equal ligands are a tie that Rules 4 and 5 might break"
        );
        let default = text(&crate::run(&built, &[1], Limits::default()).unwrap()[0]);
        let expected = if nodes > Limits::default().item_nodes {
            "unsupported:cip_digraph_limit"
        } else {
            "unsupported:cip_rule_4_or_5_needed"
        };
        assert_eq!(default, expected, "{name}");
    }
    // A pair that differs by one isotope far from the center is decided with a fraction of that.
    for (name, first, other, nodes) in [
        (
            "cubyl",
            "C12C3C4C1C5C2C3C45",
            "C12C3C4C1C5C2C3C([2H])45",
            690,
        ),
        (
            "adamantyl",
            "C12CC3CC(CC(C3)C1)C2",
            "C12CC3CC(CC(C3)C1)C2([2H])",
            312,
        ),
    ] {
        let built = parse(&format!("F[C@H](C{first})C{other}")).build();
        let report = crate::run(&built, &[1], Limits::default())
            .unwrap()
            .remove(0);
        assert_eq!(text(&report), "R", "{name}");
        assert_eq!(report.nodes, nodes, "{name}");
    }
}

#[test]
fn descriptors_nested_deeper_than_the_bound_are_a_limit_whatever_the_caps_and_the_order() {
    // Pentaethylidenecyclopentane: each descriptor needs the others, one after the other; the
    // fifth nested request is past MAX_RULE_3_NESTING and is refused as a limit, not as a cycle.
    let molecule = parse("CC=C1C(=CC)C(=CC)C(=CC)C1=CC");
    let doubles: Vec<usize> = (0..molecule.bonds.len())
        .filter(|&i| molecule.bonds[i].2 == BondOrder::Double)
        .collect();
    assert_eq!(doubles.len(), 5);
    let built = molecule.build();
    let raised = Limits {
        item_nodes: 50_000_000,
        request_nodes: 50_000_000_000,
    };
    let reversed: Vec<usize> = doubles.iter().rev().copied().collect();
    for (order, limits) in [
        (&doubles, raised),
        (&reversed, raised),
        (&doubles, Limits::default()),
    ] {
        for report in bond_reports(&built, order, limits).unwrap() {
            assert_eq!(
                state_text(report.state),
                "unsupported:cip_digraph_limit",
                "{report:?}"
            );
        }
    }
    // The raised caps leave nothing for the node count to blame: the charge is the same
    // `cap + 1` a refused nesting counts, and it is not a count of nodes materialized.
    let report = &bond_reports(&built, &doubles[..1], raised).unwrap()[0];
    assert!(report.nodes > 50_000_000, "{report:?}");
}

// ---------------------------------------------------------------------------------------
// The stack at the deepest nesting of spheres and descriptors.
// ---------------------------------------------------------------------------------------

/// Builds molecules from parts, atom by atom.
struct Parts(Molecule);

impl Parts {
    fn atom(&mut self, symbol: &str) -> usize {
        self.0.atoms.push(crate::support::element(symbol));
        self.0.atoms.len() - 1
    }

    fn bond(&mut self, a: usize, b: usize, order: BondOrder) -> usize {
        self.0.bonds.push((a, b, order));
        self.0.bonds.len() - 1
    }

    /// `levels` carbons, each with a methyl, hanging from `from`; returns the last carbon.
    /// Ordering the two children of each carbon compares the rest of the spine, so a spine
    /// nests the comparison once per level.
    fn spine(&mut self, mut from: usize, levels: usize) -> usize {
        for _ in 0..levels {
            let carbon = self.atom("C");
            self.bond(from, carbon, BondOrder::Single);
            let methyl = self.atom("C");
            self.bond(carbon, methyl, BondOrder::Single);
            from = carbon;
        }
        from
    }

    /// A stereogenic double bond hanging from `from`, stored as `relation`. At `depth` 0 its far
    /// end carries a methyl. Deeper, the far end carries two substituents that tie under Rules 1a
    /// to 2 (a spine of `levels` and a smaller group) and differ only by the geometry of the
    /// bond at the bottom of each, so its descriptor needs two descriptors of `depth - 1`.
    fn group(&mut self, from: usize, depth: usize, levels: usize, relation: Relation) {
        let near = self.atom("C");
        self.bond(from, near, BondOrder::Single);
        let far = self.atom("C");
        let index = self.bond(near, far, BondOrder::Double);
        let (begin, end) = if depth == 0 {
            let methyl = self.atom("C");
            self.bond(far, methyl, BondOrder::Single);
            (from, methyl)
        } else {
            let mut first = 0;
            for (n, inner) in [Relation::SameSide, Relation::OppositeSides]
                .into_iter()
                .enumerate()
            {
                let top = self.atom("C");
                self.bond(far, top, BondOrder::Single);
                let bottom = self.spine(top, levels);
                self.group(bottom, depth - 1, levels, inner);
                if n == 0 {
                    first = top;
                }
            }
            (from, first)
        };
        self.0.doubles.push(Double {
            bond: index,
            begin: Neighbor::Atom(begin),
            end: Neighbor::Atom(end),
            relation: Some(relation),
        });
    }
}

/// A center with fluorine, chlorine and two ligands that are the same spine of `outer` levels
/// ending in the same nest of double bonds, stored with opposite geometry at the top, so the two
/// tie under Rules 1a to 2 over the whole digraph and only the descriptors of the nest part them.
fn nest(outer: usize, depth: usize, inner: usize) -> (Molecule, usize) {
    let mut parts = Parts(Molecule::default());
    let center = parts.atom("C");
    let fluorine = parts.atom("F");
    let chlorine = parts.atom("Cl");
    parts.bond(center, fluorine, BondOrder::Single);
    parts.bond(center, chlorine, BondOrder::Single);
    let mut firsts = Vec::new();
    for relation in [Relation::SameSide, Relation::OppositeSides] {
        let head = parts.atom("C");
        parts.bond(center, head, BondOrder::Single);
        firsts.push(head);
        let bottom = parts.spine(head, outer);
        parts.group(bottom, depth, inner, relation);
    }
    parts.0.centers.push(crate::support::Center {
        atom: center,
        neighbors: [
            Neighbor::Atom(fluorine),
            Neighbor::Atom(chlorine),
            Neighbor::Atom(firsts[0]),
            Neighbor::Atom(firsts[1]),
        ],
        turn: chemifree_domain::stereo::Turn::Clockwise,
    });
    (parts.0, center)
}

/// The label of the center of [`nest`], computed on a thread with a stack of `kibibytes`.
fn nest_label(outer: usize, depth: usize, inner: usize, kibibytes: usize) -> String {
    std::thread::Builder::new()
        .stack_size(kibibytes * 1024)
        .spawn(move || {
            let (molecule, center) = nest(outer, depth, inner);
            assert!(molecule.atoms.len() <= chemifree_domain::document::MAX_ATOMS);
            let built = molecule.build();
            text(
                &crate::run(&built, &[center], Limits::default())
                    .expect("within limits")
                    .remove(0),
            )
        })
        .unwrap()
        .join()
        .expect("the labeler overflowed its stack")
}

#[test]
fn the_deepest_nesting_of_spheres_and_descriptors_fits_the_stack_of_a_worker_thread() {
    // Three nested levels of double bonds: the center asks for the descriptor of one, whose
    // ends are ranked by the descriptors of two more, and so on, so that four descriptors wait
    // on one another (MAX_RULE_3_NESTING) while the arenas that rank their ends are 25 to 32
    // spheres deep (MAX_NESTED_SPHERES). Above them a spine of tied levels takes the center's
    // own comparison as deep as the sphere bound lets it go. Measured with the stack size
    // lowered until the labeler overflowed (a debug build, the profile `cargo test` uses): the
    // nest alone needs about 0.13 MiB, a spine of 250 levels alone about 0.92 MiB, and the two
    // together (215 levels above the nest) about 0.89 MiB. Each case runs with 1.5 MiB, a
    // margin that a growth of the frames of about 60% would use up before a 2 MiB worker
    // thread overflows.
    for (outer, depth, inner, expected) in [
        (0, 3, 7, "resolved"),
        (100, 3, 7, "resolved"),
        (215, 3, 7, "resolved"),
        // One level more than the nested arenas hold: the descriptor of the top bond is a limit.
        (0, 3, 8, "unsupported:cip_digraph_limit"),
        // The center's own sphere bound.
        (250, 3, 2, "unsupported:cip_digraph_limit"),
    ] {
        let label = nest_label(outer, depth, inner, 1536);
        if expected == "resolved" {
            assert!(
                matches!(label.as_str(), "R" | "S"),
                "{outer}, {depth}, {inner}: {label}"
            );
        } else {
            assert_eq!(label, expected, "{outer}, {depth}, {inner}");
        }
    }
}

#[test]
fn a_distinction_far_from_a_double_bond_is_refused_there_and_resolved_in_a_center() {
    // The same two substituents, which differ only by a deuterium `levels` spheres away, are
    // ranked under the bound of the root they hang from: 256 spheres for the ligands of a center,
    // MAX_NESTED_SPHERES (32) for the ends of a double bond, whether the bond is labeled
    // itself or its descriptor is needed by Rule 3. The refusal is a limit, never a label.
    let far = |levels: usize| {
        let arm = format!("{}[2H]", "C(C)".repeat(levels) + "C");
        let plain = "C(C)".repeat(levels) + "C";
        (
            bond_label(&format!("CC=C({arm}){plain}"), 1, 2),
            center_label(&format!("[C@](F)(Cl)({arm}){plain}"), 0),
        )
    };
    let (bond, center) = far(20);
    assert_eq!(bond, "unspecified");
    assert!(matches!(center.as_str(), "R" | "S"), "{center}");
    let (bond, center) = far(40);
    assert_eq!(bond, "unsupported:cip_digraph_limit");
    assert!(matches!(center.as_str(), "R" | "S"), "{center}");
}

/// What a request tells of a double bond: its state and the nodes charged to it.
fn answer(report: &BondReport) -> (String, usize) {
    (state_text(report.state), report.nodes)
}

#[test]
fn the_nesting_bound_gives_every_bond_the_same_answer_in_any_request_order() {
    // `nest(0, 4, inner)` holds descriptors nested one level past MAX_RULE_3_NESTING. A bond
    // that was asked for after the descriptors below it were computed (a request in reverse
    // order) used to be answered from the memo, past the bound that the same bond asked for
    // alone or first is refused by. Whatever bonds are asked for, in whatever order, each gets
    // the state and the work it gets alone.
    let mut rng = Rng(0x00dd_ba11_cafe_f00d);
    for inner in [1, 2] {
        let (molecule, _) = nest(0, 4, inner);
        let doubles: Vec<usize> = (0..molecule.bonds.len())
            .filter(|&i| molecule.bonds[i].2 == BondOrder::Double)
            .collect();
        let built = molecule.build();
        let limits = Limits::default();
        let alone: Vec<(String, usize)> = doubles
            .iter()
            .map(|&bond| answer(&bond_reports(&built, &[bond], limits).unwrap()[0]))
            .collect();
        let refused: Vec<usize> = (0..doubles.len())
            .filter(|&i| alone[i].0 == "unsupported:cip_digraph_limit")
            .collect();
        assert!(
            refused.len() >= 2 && refused.len() < doubles.len(),
            "inner {inner}: some bonds, not all, are past the bound: {refused:?}"
        );
        let mut orders: Vec<Vec<usize>> = vec![
            (0..doubles.len()).collect(),
            (0..doubles.len()).rev().collect(),
        ];
        for _ in 0..6 {
            orders.push(rng.shuffled(doubles.len()));
        }
        for _ in 0..6 {
            let mut subset = rng.shuffled(doubles.len());
            subset.truncate(1 + rng.below(doubles.len()));
            orders.push(subset);
        }
        for order in orders {
            let asked: Vec<usize> = order.iter().map(|&i| doubles[i]).collect();
            let reports = bond_reports(&built, &asked, limits).unwrap();
            for (&i, report) in order.iter().zip(&reports) {
                assert_eq!(
                    answer(report),
                    alone[i],
                    "inner {inner}, bond {} asked in the order {order:?}",
                    doubles[i]
                );
            }
        }
    }
}
