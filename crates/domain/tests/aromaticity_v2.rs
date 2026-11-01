//! Model revision 2: fused partially aromatic systems, inferred-hydrogen nitrogen roles,
//! bounded work, and chemical identity consequences.

use chemifree_domain::aromaticity::{self, Classification, KekuleOrder};
use chemifree_domain::document::{
    Atom, AtomRef, BondOrder, Command, Document, DocumentId, NewAtomPositions, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::identity::{self, IdentityError};
use chemifree_domain::states::{Hydrogens, Radical};

/// Atoms are written `C`, `N` (inferred hydrogens) or `NH1` (stated hydrogen count).
/// Bonds are `-` single, `=` double and `:` aromatic.
fn mol(atoms: &[&str], bonds: &[(usize, usize, char)]) -> MoleculeGraph {
    let mut document = Document::new(DocumentId::parse("v2-test").unwrap(), "v2").unwrap();
    let mut commands: Vec<Command> = atoms
        .iter()
        .map(|spec| {
            // A trailing `.` marks a doublet radical and `!` a singlet (carbene).
            let (spec, radical) = match spec.as_bytes().last() {
                Some(b'.') => (&spec[..spec.len() - 1], Radical::Doublet),
                Some(b'!') => (&spec[..spec.len() - 1], Radical::Singlet),
                _ => (&spec[..], Radical::None),
            };
            let (symbol, hydrogens) = match spec.split_once('H') {
                Some((symbol, count)) => (symbol, Some(count.parse::<u8>().unwrap())),
                None => (spec, None),
            };
            let mut atom = Atom::from(Element::from_symbol(symbol).unwrap());
            atom.radical = radical;
            if let Some(count) = hydrogens {
                atom.hydrogens = Hydrogens::Specified(count);
            }
            Command::AddAtom {
                atom,
                positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
                bond_to: None,
            }
        })
        .collect();
    commands.extend(bonds.iter().map(|&(a, b, kind)| Command::AddBond {
        begin: AtomRef::Created(a),
        end: AtomRef::Created(b),
        order: match kind {
            '-' => BondOrder::Single,
            '=' => BondOrder::Double,
            ':' => BondOrder::Aromatic,
            other => panic!("unknown bond kind {other}"),
        },
    }));
    for chunk in commands.chunks(500) {
        document.apply(chunk).unwrap();
    }
    MoleculeGraph::from_document(&document)
}

/// Name, atoms beyond the benzene, and the bonds closing the second ring.
type Case = (&'static str, Vec<&'static str>, Vec<(usize, usize, char)>);

fn kinds(system: &aromaticity::System) -> (Classification, Option<&'static str>) {
    (system.classification, system.reason)
}

fn only(graph: &MoleculeGraph) -> (Classification, Option<&'static str>) {
    let result = aromaticity::perceive(graph);
    assert_eq!(result.systems.len(), 1, "expected one ring system");
    kinds(&result.systems[0])
}

/// A fused benzene plus a second ring. `ring` lists the atoms beyond the six benzene
/// atoms (0..6, shared edge 0-5 closes the second ring); `kekule` chooses the spelling.
fn benzo(extra: &[&str], ring: &[(usize, usize, char)], kekule: bool) -> MoleculeGraph {
    let mut atoms = vec!["C"; 6];
    atoms.extend_from_slice(extra);
    let mut bonds: Vec<(usize, usize, char)> = if kekule {
        vec![
            (0, 1, '='),
            (1, 2, '-'),
            (2, 3, '='),
            (3, 4, '-'),
            (4, 5, '='),
            (5, 0, '-'),
        ]
    } else {
        (0..6).map(|i| (i, (i + 1) % 6, ':')).collect()
    };
    bonds.extend_from_slice(ring);
    mol(&atoms, &bonds)
}

#[test]
fn benzene_fused_to_saturated_or_nonconjugated_rings_is_unsupported_in_both_spellings() {
    // Each case: extra atoms and the bonds closing the second ring across the 0-5 edge.
    let cases: [Case; 5] = [
        (
            "indane",
            vec!["C", "C", "C"],
            vec![(0, 6, '-'), (6, 7, '-'), (7, 8, '-'), (8, 5, '-')],
        ),
        (
            "indene",
            vec!["C", "C", "C"],
            vec![(0, 6, '-'), (6, 7, '='), (7, 8, '-'), (8, 5, '-')],
        ),
        (
            "benzodioxole",
            vec!["O", "C", "O"],
            vec![(0, 6, '-'), (6, 7, '-'), (7, 8, '-'), (8, 5, '-')],
        ),
        (
            "indoline",
            vec!["N", "C", "C"],
            vec![(0, 6, '-'), (6, 7, '-'), (7, 8, '-'), (8, 5, '-')],
        ),
        (
            "tetralin",
            vec!["C", "C", "C", "C"],
            vec![
                (0, 6, '-'),
                (6, 7, '-'),
                (7, 8, '-'),
                (8, 9, '-'),
                (9, 5, '-'),
            ],
        ),
    ];
    for (name, extra, ring) in cases {
        for kekule in [true, false] {
            let graph = benzo(&extra, &ring, kekule);
            assert_eq!(
                only(&graph),
                (
                    Classification::Unsupported,
                    Some("fused_partially_aromatic")
                ),
                "{name} kekule={kekule}"
            );
        }
    }
}

#[test]
fn fluorene_like_system_with_two_benzenes_and_a_methylene_is_unsupported() {
    // Benzene A 0-5, benzene B 6-11, CH2 atom 12 bridges 0 and 6; 5-11 are bonded.
    let mut bonds: Vec<(usize, usize, char)> = Vec::new();
    for base in [0usize, 6] {
        for i in 0..6 {
            bonds.push((
                base + i,
                base + (i + 1) % 6,
                if i % 2 == 0 { '=' } else { '-' },
            ));
        }
    }
    bonds.extend([(5, 11, '-'), (0, 12, '-'), (12, 6, '-')]);
    let graph = mol(&["C"; 13], &bonds);
    assert_eq!(
        only(&graph),
        (
            Classification::Unsupported,
            Some("fused_partially_aromatic")
        )
    );
}

#[test]
fn nonconjugated_bicyclic_alkenes_remain_nonaromatic() {
    // Norbornene: C1=CC2CCC1C2.
    let norbornene = mol(
        &["C"; 7],
        &[
            (0, 1, '='),
            (1, 2, '-'),
            (2, 3, '-'),
            (3, 4, '-'),
            (4, 0, '-'),
            (4, 5, '-'),
            (5, 2, '-'),
        ],
    );
    assert_eq!(
        only(&norbornene),
        (Classification::NonAromatic, Some("not_conjugated"))
    );
    // alpha-pinene: CC1=CCC2CC1C2(C)C, with a quaternary ring carbon.
    let pinene = mol(
        &["C"; 10],
        &[
            (0, 1, '-'),
            (1, 2, '='),
            (2, 3, '-'),
            (3, 4, '-'),
            (4, 5, '-'),
            (5, 1, '-'),
            (5, 6, '-'),
            (6, 4, '-'),
            (6, 7, '-'),
            (6, 8, '-'),
        ],
    );
    let result = aromaticity::perceive(&pinene);
    assert_eq!(result.systems.len(), 1);
    assert_eq!(
        kinds(&result.systems[0]),
        (Classification::NonAromatic, Some("not_conjugated"))
    );
}

#[test]
fn kekule_ring_nitrogen_with_inferred_hydrogen_is_pyrrole_type() {
    let pyrrole = mol(
        &["N", "C", "C", "C", "C"],
        &[
            (0, 1, '-'),
            (1, 2, '='),
            (2, 3, '-'),
            (3, 4, '='),
            (4, 0, '-'),
        ],
    );
    let result = aromaticity::perceive(&pyrrole);
    assert_eq!(
        (
            result.systems[0].classification,
            result.systems[0].electrons
        ),
        (Classification::Aromatic, Some(6))
    );
    // Pyrazole N1-N2: the first nitrogen donates, the second is pyridine-type.
    let pyrazole = mol(
        &["N", "N", "C", "C", "C"],
        &[
            (0, 1, '-'),
            (1, 2, '='),
            (2, 3, '-'),
            (3, 4, '='),
            (4, 0, '-'),
        ],
    );
    assert_eq!(only(&pyrazole), (Classification::Aromatic, None));
    // Imidazole.
    let imidazole = mol(
        &["N", "C", "N", "C", "C"],
        &[
            (0, 1, '-'),
            (1, 2, '='),
            (2, 3, '-'),
            (3, 4, '='),
            (4, 0, '-'),
        ],
    );
    assert_eq!(only(&imidazole), (Classification::Aromatic, None));
    // The same ring written with aromatic bonds and a stated hydrogen agrees.
    let aromatic = mol(
        &["NH1", "C", "C", "C", "C"],
        &[
            (0, 1, ':'),
            (1, 2, ':'),
            (2, 3, ':'),
            (3, 4, ':'),
            (4, 0, ':'),
        ],
    );
    assert_eq!(only(&aromatic), (Classification::Aromatic, None));
}

#[test]
fn aromatic_nitrogen_without_stated_hydrogen_stays_pyridine_type() {
    let ring = [
        (0, 1, ':'),
        (1, 2, ':'),
        (2, 3, ':'),
        (3, 4, ':'),
        (4, 0, ':'),
    ];
    let graph = mol(&["N", "C", "C", "C", "C"], &ring);
    let result = aromaticity::perceive(&graph);
    assert_eq!(
        (
            result.systems[0].classification,
            result.systems[0].electrons,
            result.systems[0].reason
        ),
        (Classification::NonAromatic, Some(5), Some("electron_count"))
    );
}

#[test]
fn fused_heteroaromatics_get_the_same_answer_in_both_spellings() {
    // Indole: benzene 0-5 fused to N6 C7 C8 across 0-5.
    let kekule = benzo(
        &["N", "C", "C"],
        &[(0, 6, '-'), (6, 7, '-'), (7, 8, '='), (8, 5, '-')],
        true,
    );
    let aromatic = benzo(
        &["NH1", "C", "C"],
        &[(0, 6, ':'), (6, 7, ':'), (7, 8, ':'), (8, 5, ':')],
        false,
    );
    // Bond 5-0 is shared; both spellings must be undecided in the same way.
    assert_eq!(only(&kekule), only(&aromatic));
    assert_eq!(only(&kekule).0, Classification::Unsupported);
}

#[test]
fn chemical_identity_agrees_between_kekule_and_aromatic_pyrrole() {
    let kekule = mol(
        &["N", "C", "C", "C", "C"],
        &[
            (0, 1, '-'),
            (1, 2, '='),
            (2, 3, '-'),
            (3, 4, '='),
            (4, 0, '-'),
        ],
    );
    let aromatic = mol(
        &["NH1", "C", "C", "C", "C"],
        &[
            (0, 1, ':'),
            (1, 2, ':'),
            (2, 3, ':'),
            (3, 4, ':'),
            (4, 0, ':'),
        ],
    );
    let a = identity::canonicalize_chemical(&kekule).unwrap();
    let b = identity::canonicalize_chemical(&aromatic).unwrap();
    assert_eq!(a.code, b.code);
}

#[test]
fn chemical_identity_refusals_keep_their_specific_reasons() {
    let tetralin = benzo(
        &["C"; 4],
        &[
            (0, 6, '-'),
            (6, 7, '-'),
            (7, 8, '-'),
            (8, 9, '-'),
            (9, 5, '-'),
        ],
        true,
    );
    assert_eq!(
        identity::canonicalize_chemical(&tetralin).unwrap_err(),
        IdentityError::Unsupported("fused_partially_aromatic")
    );
    // An aromatic bond in a ring the model classifies non-aromatic.
    let five = mol(
        &["N", "C", "C", "C", "C"],
        &[
            (0, 1, ':'),
            (1, 2, ':'),
            (2, 3, ':'),
            (3, 4, ':'),
            (4, 0, ':'),
        ],
    );
    assert_eq!(
        identity::canonicalize_chemical(&five).unwrap_err(),
        IdentityError::Unsupported("aromaticity_unsupported")
    );
    // An aromatic bond that belongs to no ring.
    let chain = mol(&["C", "C"], &[(0, 1, ':')]);
    assert_eq!(
        identity::canonicalize_chemical(&chain).unwrap_err(),
        IdentityError::Unsupported("aromaticity_unsupported")
    );
    // A ring larger than the system cap.
    let big = mol(
        &["C"; 129],
        &(0..129)
            .map(|i| (i, (i + 1) % 129, ':'))
            .collect::<Vec<_>>(),
    );
    assert_eq!(
        identity::canonicalize_chemical(&big).unwrap_err(),
        IdentityError::AtomLimit
    );
    // A valence failure outside any ring is a valence reason, not an aromaticity one.
    let overvalent = mol(
        &["C", "C", "C", "C", "C", "C"],
        &[
            (0, 1, '-'),
            (0, 2, '-'),
            (0, 3, '-'),
            (0, 4, '-'),
            (0, 5, '-'),
        ],
    );
    assert_eq!(
        identity::canonicalize_chemical(&overvalent).unwrap_err(),
        IdentityError::Unsupported("unusual_valence")
    );
}

#[test]
fn large_decidable_systems_stay_decided_at_the_size_cap() {
    // A ladder of 64 rungs has 128 atoms and 63 fused four-membered rings.
    let ladder = |rung: char, rail: char| {
        let n = 64;
        let mut bonds: Vec<(usize, usize, char)> =
            (0..n).map(|i| (2 * i, 2 * i + 1, rung)).collect();
        for i in 0..n - 1 {
            bonds.push((2 * i, 2 * i + 2, rail));
            bonds.push((2 * i + 1, 2 * i + 3, rail));
        }
        mol(&vec!["C"; 2 * n], &bonds)
    };
    assert_eq!(
        only(&ladder('-', '-')),
        (Classification::NonAromatic, Some("not_conjugated"))
    );
    assert_eq!(
        only(&ladder('=', '-')),
        (
            Classification::Unsupported,
            Some("fused_system_outside_model")
        )
    );
}

#[test]
fn a_126_atom_acene_in_aromatic_spelling_is_decided_within_budget() {
    // 31 linearly fused rings: rails 0..=62 on top and 63.. on bottom, rungs between.
    let rings = 31;
    let columns = 2 * rings + 1;
    let mut bonds: Vec<(usize, usize, char)> = Vec::new();
    for c in 0..columns - 1 {
        bonds.push((c, c + 1, ':'));
        bonds.push((columns + c, columns + c + 1, ':'));
    }
    for c in (0..columns).step_by(2) {
        bonds.push((c, columns + c, ':'));
    }
    let graph = mol(&vec!["C"; 2 * columns], &bonds);
    let result = aromaticity::perceive(&graph);
    assert_eq!(result.systems.len(), 1);
    assert_eq!(result.systems[0].atoms.len(), 126);
    assert_eq!(kinds(&result.systems[0]), (Classification::Aromatic, None));
    assert!(result.kekulize().is_ok());
}

#[test]
fn aromatic_conversion_plans_a_hydrogen_only_for_inferred_pyrrole_type_nitrogen() {
    let kekule = mol(
        &["N", "C", "C", "C", "C"],
        &[
            (0, 1, '-'),
            (1, 2, '='),
            (2, 3, '-'),
            (3, 4, '='),
            (4, 0, '-'),
        ],
    );
    let plan = aromaticity::perceive(&kekule).aromatize().unwrap();
    assert_eq!(plan.bonds.len(), 5);
    assert_eq!(plan.hydrogens.len(), 1);
    assert_eq!(plan.hydrogens[0].1, 1);
    // A stated hydrogen, a pyridine-type nitrogen and an oxygen need no statement.
    let stated = mol(
        &["NH1", "C", "C", "C", "C"],
        &[
            (0, 1, '-'),
            (1, 2, '='),
            (2, 3, '-'),
            (3, 4, '='),
            (4, 0, '-'),
        ],
    );
    assert!(
        aromaticity::perceive(&stated)
            .aromatize()
            .unwrap()
            .hydrogens
            .is_empty()
    );
    let pyridine = mol(
        &["N", "C", "C", "C", "C", "C"],
        &[
            (0, 1, '='),
            (1, 2, '-'),
            (2, 3, '='),
            (3, 4, '-'),
            (4, 5, '='),
            (5, 0, '-'),
        ],
    );
    assert!(
        aromaticity::perceive(&pyridine)
            .aromatize()
            .unwrap()
            .hydrogens
            .is_empty()
    );
    let furan = mol(
        &["O", "C", "C", "C", "C"],
        &[
            (0, 1, '-'),
            (1, 2, '='),
            (2, 3, '-'),
            (3, 4, '='),
            (4, 0, '-'),
        ],
    );
    assert!(
        aromaticity::perceive(&furan)
            .aromatize()
            .unwrap()
            .hydrogens
            .is_empty()
    );
}

#[test]
fn kekule_conversion_resets_only_nitrogen_counts_that_match_the_valence_model() {
    let ring = [
        (0, 1, ':'),
        (1, 2, ':'),
        (2, 3, ':'),
        (3, 4, ':'),
        (4, 0, ':'),
    ];
    let matching = mol(&["NH1", "C", "C", "C", "C"], &ring);
    let assignment = aromaticity::kekulize(&matching).unwrap();
    assert_eq!(
        aromaticity::redundant_nitrogen_hydrogens(&matching, &assignment).len(),
        1
    );
    // A six-membered nitrogen with a stated hydrogen would be valence 4: its count differs
    // from the inferred 0, so it is kept (the model does not accept this ring, so the
    // assignment is supplied directly).
    let pyridine = mol(
        &["NH1", "C", "C", "C", "C", "C"],
        &[
            (0, 1, '-'),
            (1, 2, '-'),
            (2, 3, '-'),
            (3, 4, '-'),
            (4, 5, '-'),
            (5, 0, '-'),
        ],
    );
    let doubles = [(
        aromaticity::perceive(&pyridine).systems[0].bonds[0],
        KekuleOrder::Double,
    )];
    assert!(aromaticity::redundant_nitrogen_hydrogens(&pyridine, &doubles).is_empty());
    // A stated count on a carbon is never touched.
    let carbon = mol(
        &["C", "C", "C", "C", "C", "C"],
        &(0..6).map(|i| (i, (i + 1) % 6, ':')).collect::<Vec<_>>(),
    );
    let assignment = aromaticity::kekulize(&carbon).unwrap();
    assert!(aromaticity::redundant_nitrogen_hydrogens(&carbon, &assignment).is_empty());
}

#[test]
fn identity_refuses_each_reachable_aromatic_failure_with_the_models_reason() {
    let seven = mol(
        &["NH1", "C", "NH1", "C", "NH1", "C", "C"],
        &(0..7).map(|i| (i, (i + 1) % 7, ':')).collect::<Vec<_>>(),
    );
    assert_eq!(
        identity::canonicalize_chemical(&seven).unwrap_err(),
        IdentityError::Unsupported("no_kekule_assignment")
    );
    let with_unknown = {
        let mut document = Document::new(DocumentId::parse("unknown-ring").unwrap(), "u").unwrap();
        let mut commands: Vec<Command> = (0..6)
            .map(|_| Command::AddAtom {
                atom: Atom::from(Element::from_symbol("C").unwrap()),
                positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
                bond_to: None,
            })
            .collect();
        commands.extend((0..6).map(|i| Command::AddBond {
            begin: AtomRef::Created(i),
            end: AtomRef::Created((i + 1) % 6),
            order: if i == 0 {
                BondOrder::Unknown
            } else {
                BondOrder::Aromatic
            },
        }));
        document.apply(&commands).unwrap();
        MoleculeGraph::from_document(&document)
    };
    assert_eq!(
        identity::canonicalize_chemical(&with_unknown).unwrap_err(),
        IdentityError::Unsupported("unsupported_ring_bond")
    );
}

// ---- Review round 1 ----

fn ring_bonds(n: usize, kind: char) -> Vec<(usize, usize, char)> {
    (0..n).map(|i| (i, (i + 1) % n, kind)).collect()
}

#[test]
fn saturated_ring_atoms_are_validated_before_they_are_peeled() {
    // A cyclohexane carbon stated as CH3 has valence 5.
    let stated = mol(&["CH3", "C", "C", "C", "C", "C"], &ring_bonds(6, '-'));
    assert_eq!(
        only(&stated),
        (Classification::Unsupported, Some("valence_not_in_model"))
    );
    // A ring carbon with three extra substituents has valence 5.
    let mut bonds = ring_bonds(6, '-');
    bonds.extend([(0, 6, '-'), (0, 7, '-'), (0, 8, '-')]);
    let crowded = mol(&["C"; 9], &bonds);
    assert_eq!(
        only(&crowded),
        (Classification::Unsupported, Some("valence_not_in_model"))
    );
    // The refusal reaches conversion: a valid benzene elsewhere is not converted alone.
    let mut atoms = vec!["C"; 6];
    atoms.extend(["CH3", "C", "C", "C", "C", "C"]);
    let mut bonds: Vec<(usize, usize, char)> = (0..6)
        .map(|i| (i, (i + 1) % 6, if i % 2 == 0 { '=' } else { '-' }))
        .collect();
    bonds.extend((0..6).map(|i| (6 + i, 6 + (i + 1) % 6, '-')));
    let both = mol(&atoms, &bonds);
    assert_eq!(
        aromaticity::perceive(&both)
            .aromatize()
            .map(|plan| plan.bonds),
        Err("valence_not_in_model")
    );
}

#[test]
fn rings_without_a_pi_bond_are_not_conjugated_even_when_every_atom_donates() {
    // Cyclotriazane: three NH donors, only sigma bonds.
    let triazane = mol(&["N", "N", "N"], &ring_bonds(3, '-'));
    assert_eq!(
        only(&triazane),
        (Classification::NonAromatic, Some("not_conjugated"))
    );
    // Cyclopentane analogue made of sulfur and oxygen donors.
    let donors = mol(&["O", "S", "O", "S", "O"], &ring_bonds(5, '-'));
    assert_eq!(
        only(&donors),
        (Classification::NonAromatic, Some("not_conjugated"))
    );
    // Adjacent donors beside one C=C are not a conjugated circuit the model can read.
    let diazete = mol(
        &["N", "N", "C", "C"],
        &[(0, 1, '-'), (1, 2, '-'), (2, 3, '='), (3, 0, '-')],
    );
    assert_eq!(
        only(&diazete),
        (Classification::Unsupported, Some("adjacent_donors"))
    );
}

#[test]
fn a_donor_only_ring_in_a_fused_block_is_not_a_candidate_aromatic_ring() {
    // N0-N1-N2 ring sharing the N0-N1 edge with a ring through two CH2 groups.
    let block = mol(
        &["N", "N", "N", "C", "C"],
        &[
            (0, 1, '-'),
            (1, 2, '-'),
            (2, 0, '-'),
            (0, 3, '-'),
            (3, 4, '-'),
            (4, 1, '-'),
        ],
    );
    assert_eq!(
        only(&block),
        (Classification::NonAromatic, Some("not_conjugated"))
    );
}

#[test]
fn pyridinium_and_methylpyridinium_nitrogen_hydrogens_are_not_reset() {
    // Aromatic pyridinium [nH+]: charged, so its stated count is not what conversion restores.
    let mut document = Document::new(DocumentId::parse("pyridinium").unwrap(), "p").unwrap();
    let mut atoms: Vec<Atom> = (0..6)
        .map(|_| Atom::from(Element::from_symbol("C").unwrap()))
        .collect();
    atoms[0] = Atom::from(Element::from_symbol("N").unwrap());
    atoms[0].charge = 1;
    atoms[0].hydrogens = Hydrogens::Specified(1);
    let mut commands: Vec<Command> = atoms
        .into_iter()
        .map(|atom| Command::AddAtom {
            atom,
            positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
            bond_to: None,
        })
        .collect();
    commands.extend((0..6).map(|i| Command::AddBond {
        begin: AtomRef::Created(i),
        end: AtomRef::Created((i + 1) % 6),
        order: BondOrder::Aromatic,
    }));
    document.apply(&commands).unwrap();
    let graph = MoleculeGraph::from_document(&document);
    let assignment = aromaticity::kekulize(&graph).unwrap();
    assert!(aromaticity::redundant_nitrogen_hydrogens(&graph, &assignment).is_empty());
}

#[test]
fn unstated_aromatic_nitrogen_hydrogen_differs_from_the_kekule_spelling_by_design() {
    let kekule = mol(
        &["N", "C", "C", "C", "C"],
        &[
            (0, 1, '-'),
            (1, 2, '='),
            (2, 3, '-'),
            (3, 4, '='),
            (4, 0, '-'),
        ],
    );
    let aromatic = mol(&["N", "C", "C", "C", "C"], &ring_bonds(5, ':'));
    assert_eq!(only(&kekule).0, Classification::Aromatic);
    assert_eq!(
        only(&aromatic),
        (Classification::NonAromatic, Some("electron_count"))
    );
    assert!(identity::canonicalize_chemical(&kekule).is_ok());
    assert_eq!(
        identity::canonicalize_chemical(&aromatic).unwrap_err(),
        IdentityError::Unsupported("aromaticity_unsupported")
    );
}

// ---- Review round 2 ----

const KEKULE_SIX: [(usize, usize, char); 6] = [
    (0, 1, '='),
    (1, 2, '-'),
    (2, 3, '='),
    (3, 4, '-'),
    (4, 5, '='),
    (5, 0, '-'),
];

#[test]
fn radical_ring_atoms_are_unsupported_in_both_spellings() {
    // A doublet carbon in Kekule benzene.
    let radical_benzene = mol(&["C.", "C", "C", "C", "C", "C"], &KEKULE_SIX);
    assert_eq!(
        only(&radical_benzene),
        (Classification::Unsupported, Some("radical_state"))
    );
    // Cyclopentadienyl radical.
    let cyclopentadienyl = mol(
        &["C.", "C", "C", "C", "C"],
        &[
            (0, 1, '-'),
            (1, 2, '='),
            (2, 3, '-'),
            (3, 4, '='),
            (4, 0, '-'),
        ],
    );
    assert_eq!(
        only(&cyclopentadienyl),
        (Classification::Unsupported, Some("radical_state"))
    );
    // A doublet-radical aromatic pyridine nitrogen.
    let radical_pyridine = mol(&["N.", "C", "C", "C", "C", "C"], &ring_bonds(6, ':'));
    assert_eq!(
        only(&radical_pyridine),
        (Classification::Unsupported, Some("radical_state"))
    );
    // Cycloheptatrienylidene: a singlet carbene closing a triene.
    let carbene = mol(
        &["C!", "C", "C", "C", "C", "C", "C"],
        &[
            (0, 1, '-'),
            (1, 2, '='),
            (2, 3, '-'),
            (3, 4, '='),
            (4, 5, '-'),
            (5, 6, '='),
            (6, 0, '-'),
        ],
    );
    assert_eq!(
        only(&carbene),
        (Classification::Unsupported, Some("radical_state"))
    );
    // A saturated radical or carbene in a block is not peeled away.
    let saturated = mol(&["C.", "C", "C", "C", "C", "C"], &ring_bonds(6, '-'));
    assert_eq!(
        only(&saturated),
        (Classification::Unsupported, Some("radical_state"))
    );
    assert_eq!(
        identity::canonicalize_chemical(&radical_benzene).unwrap_err(),
        IdentityError::Unsupported("radical_state")
    );
}

#[test]
fn hypervalent_donors_get_the_same_refusal_in_both_spellings() {
    let kekule_bonds = [
        (0, 1, '-'),
        (1, 2, '='),
        (2, 3, '-'),
        (3, 4, '='),
        (4, 0, '-'),
    ];
    let aromatic_bonds = ring_bonds(5, ':');
    let refused = (Classification::Unsupported, Some("valence_not_in_model"));
    // Ring N with two extra single bonds (valence 5).
    for (label, bonds) in [
        ("kekule", &kekule_bonds[..]),
        ("aromatic", &aromatic_bonds[..]),
    ] {
        let mut atoms = vec!["N", "C", "C", "C", "C", "C", "C"];
        let mut all = bonds.to_vec();
        all.extend([(0, 5, '-'), (0, 6, '-')]);
        let graph = mol(&atoms, &all);
        assert_eq!(only(&graph), refused, "N with two extra bonds, {label}");
        // N stated Specified(2) with three single bonds.
        atoms = vec!["NH2", "C", "C", "C", "C", "C"];
        let mut all = bonds.to_vec();
        all.push((0, 5, '-'));
        let graph = mol(&atoms, &all);
        assert_eq!(only(&graph), refused, "NH2 with three bonds, {label}");
        // Thiophene S with two exocyclic single bonds to O (valence 4).
        let atoms = vec!["S", "C", "C", "C", "C", "O", "O"];
        let mut all = bonds.to_vec();
        all.extend([(0, 5, '-'), (0, 6, '-')]);
        let graph = mol(&atoms, &all);
        assert_eq!(only(&graph), refused, "S with two exocyclic bonds, {label}");
    }
}
