use chemifree_domain::chemistry::{Finding, inspect_valence};
use chemifree_domain::document::{
    Atom, AtomRef, BondOrder, Command, Document, DocumentId, NewAtomPositions, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::states::{AtomKind, Hydrogens, QueryAtom, Radical};

fn element(symbol: &str) -> Atom {
    Atom::from(Element::from_symbol(symbol).unwrap())
}
fn drawing(atoms: Vec<Atom>, bonds: &[(usize, usize, BondOrder)]) -> Document {
    let mut doc = Document::new(
        DocumentId::parse("valence-example").unwrap(),
        "Valence example",
    )
    .unwrap();
    let mut commands: Vec<_> = atoms
        .into_iter()
        .map(|atom| Command::AddAtom {
            atom,
            positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
            bond_to: None,
        })
        .collect();
    commands.extend(bonds.iter().map(|(a, b, order)| Command::AddBond {
        begin: AtomRef::Created(*a),
        end: AtomRef::Created(*b),
        order: order.clone(),
    }));
    doc.apply(&commands).unwrap();
    doc
}

#[test]
fn methane_and_explicit_hydrogen_agree_without_rewriting_input() {
    let methane = drawing(vec![element("C")], &[]);
    let read = inspect_valence(&methane);
    assert_eq!(
        (
            read[0].finding,
            read[0].bond_order_sum,
            read[0].hydrogen_count
        ),
        (Finding::Valid, Some(0), Some(4))
    );
    let bonds: Vec<_> = (1..=4).map(|i| (0, i, BondOrder::Single)).collect();
    let mut carbon = element("C");
    carbon.hydrogens = Hydrogens::Specified(0);
    let mut atoms = vec![carbon.clone()];
    atoms.extend((0..4).map(|_| element("H")));
    let doc = drawing(atoms.clone(), &bonds);
    let before = doc.clone();
    let result = inspect_valence(&doc);
    assert_eq!(
        (
            result[0].finding,
            result[0].bond_order_sum,
            result[0].hydrogen_count
        ),
        (Finding::Valid, Some(4), Some(0))
    );
    assert!(result.iter().all(|a| a.finding == Finding::Valid));
    assert_eq!(doc, before);
    atoms[0].hydrogens = Hydrogens::Specified(1);
    let over = drawing(atoms, &bonds);
    let before = over.clone();
    assert_eq!(
        (
            inspect_valence(&over)[0].finding,
            inspect_valence(&over)[0].reason
        ),
        (Finding::Invalid, Some("unusual_valence"))
    );
    assert_eq!(over, before);
}

#[test]
fn charge_radical_and_isotope_are_interpreted_explicitly() {
    let mut ammonium = element("N");
    ammonium.charge = 1;
    let mut methyl = element("C");
    methyl.radical = Radical::Doublet;
    let mut isotope = element("C");
    isotope.isotope = Some(13);
    let doc = drawing(
        vec![element("N"), ammonium, methyl, element("C"), isotope],
        &[],
    );
    let result = inspect_valence(&doc);
    assert_eq!(
        result
            .iter()
            .map(|a| (a.finding, a.hydrogen_count))
            .collect::<Vec<_>>(),
        [
            (Finding::Valid, Some(3)),
            (Finding::Valid, Some(4)),
            (Finding::Valid, Some(3)),
            (Finding::Valid, Some(4)),
            (Finding::Valid, Some(4)),
        ]
    );
}

#[test]
fn uncertain_atoms_and_bonds_never_become_single_bond_guesses() {
    let mut unknown = element("C");
    unknown.hydrogens = Hydrogens::Unknown;
    let mut radical = element("O");
    radical.radical = Radical::Unknown;
    let query = Atom::new(AtomKind::Query(QueryAtom::Any));
    let doc = drawing(
        vec![
            unknown,
            radical,
            query,
            element("Fe"),
            element("C"),
            element("C"),
        ],
        &[(4, 5, BondOrder::Dative)],
    );
    let before = doc.clone();
    let assessed = inspect_valence(&doc);
    assert_eq!(
        assessed.iter().map(|a| a.reason).collect::<Vec<_>>(),
        [
            Some("unknown_hydrogens"),
            Some("radical_state"),
            Some("query_atom"),
            Some("element_or_charge"),
            Some("non_integral_or_unknown_bond"),
            Some("non_integral_or_unknown_bond"),
        ]
    );
    assert!(assessed.iter().all(|a| a.finding == Finding::Unsupported));
    assert_eq!(assessed[4].bond_order_sum, None);
    assert_eq!(doc, before);
}

fn ion(symbol: &str, charge: i8) -> Atom {
    let mut atom = element(symbol);
    atom.charge = charge;
    atom
}

#[test]
fn simple_monatomic_ions_have_valence_zero() {
    let ions = [
        ("Li", 1),
        ("Na", 1),
        ("K", 1),
        ("Mg", 2),
        ("Ca", 2),
        ("F", -1),
        ("Cl", -1),
        ("Br", -1),
        ("I", -1),
    ];
    let doc = drawing(ions.iter().map(|(s, c)| ion(s, *c)).collect(), &[]);
    for ((symbol, charge), read) in ions.iter().zip(inspect_valence(&doc)) {
        assert_eq!(
            (
                read.finding,
                read.reason,
                read.bond_order_sum,
                read.hydrogen_count
            ),
            (Finding::Valid, None, Some(0), Some(0)),
            "{symbol}{charge:+}"
        );
    }
}

#[test]
fn only_the_listed_ions_are_added_to_the_valence_model() {
    // Other charges of the same elements, other metals, and heavier group members stay
    // outside the model; so do radicals on the listed ions.
    let unlisted = [
        ("Na", -1),
        ("Na", 2),
        ("Mg", 1),
        ("Mg", 0),
        ("Ca", 1),
        ("Li", 0),
        ("K", 2),
        ("Rb", 1),
        ("Cs", 1),
        ("Be", 2),
        ("Ba", 2),
        ("Al", 3),
        ("Fe", 2),
        ("Fe", 3),
        ("Zn", 2),
        ("Cu", 1),
    ];
    let doc = drawing(unlisted.iter().map(|(s, c)| ion(s, *c)).collect(), &[]);
    for ((symbol, charge), read) in unlisted.iter().zip(inspect_valence(&doc)) {
        assert_eq!(
            (read.finding, read.reason),
            (Finding::Unsupported, Some("element_or_charge")),
            "{symbol}{charge:+}"
        );
    }
    let mut radical = ion("Na", 1);
    radical.radical = Radical::Doublet;
    let read = inspect_valence(&drawing(vec![radical], &[]));
    assert_eq!(
        (read[0].finding, read[0].reason),
        (Finding::Unsupported, Some("radical_state"))
    );
}

#[test]
fn a_bonded_listed_ion_is_an_unusual_valence_not_an_unknown_state() {
    let doc = drawing(
        vec![ion("Na", 1), element("C")],
        &[(0, 1, BondOrder::Single)],
    );
    let read = inspect_valence(&doc);
    assert_eq!(
        (read[0].finding, read[0].reason, read[0].bond_order_sum),
        (Finding::Invalid, Some("unusual_valence"), Some(1))
    );
}

#[test]
fn the_model_id_names_the_table_that_includes_the_ions() {
    assert_eq!(
        chemifree_domain::chemistry::VALENCE_MODEL,
        "chemifree-valence-3"
    );
}

#[test]
fn restricted_target_is_the_ordinary_valence_the_aromaticity_model_accepts() {
    use chemifree_domain::chemistry::restricted_target;
    // Expected totals come from the ring-atom table of the aromaticity model.
    let expected = [
        ("C", 0, 4),
        ("C", 1, 3),
        ("C", -1, 3),
        ("N", 0, 3),
        ("N", 1, 4),
        ("N", -1, 2),
        ("O", 0, 2),
        ("S", 0, 2),
    ];
    for (symbol, charge, target) in expected {
        assert_eq!(
            restricted_target(symbol, charge),
            Some(target),
            "{symbol} charge {charge}"
        );
    }
    // The valence model lists higher states for N and S; the restricted lookup excludes them.
    assert_eq!(restricted_target("Xx", 0), None);
    assert_eq!(restricted_target("O", 5), None);
}

#[test]
fn restricted_target_lists_the_ions_but_the_ring_model_never_takes_them() {
    use chemifree_domain::aromaticity::{self, Classification};
    use chemifree_domain::chemistry::restricted_target;
    use chemifree_domain::graph::MoleculeGraph;
    use chemifree_domain::identity::{self, IdentityError};
    // The shared lookup answers valence 0 for the listed ions ...
    for (symbol, charge) in [
        ("Li", 1),
        ("Na", 1),
        ("K", 1),
        ("Mg", 2),
        ("Ca", 2),
        ("Cl", -1),
    ] {
        assert_eq!(
            restricted_target(symbol, charge),
            Some(0),
            "{symbol}{charge:+}"
        );
    }
    assert_eq!(restricted_target("Na", 2), None);
    // ... and the ring model, which has its own table of ring atoms, still refuses a ring
    // that holds one, in the Kekulé and in the aromatic spelling, whichever ion it is.
    for (symbol, charge) in [
        ("Na", 1),
        ("Li", 1),
        ("K", 1),
        ("Mg", 2),
        ("Ca", 2),
        ("Cl", -1),
    ] {
        for (order, reason) in [
            (BondOrder::Single, "valence_not_in_model"),
            (BondOrder::Aromatic, "element_or_charge"),
        ] {
            let mut atoms = vec![element("C"); 5];
            atoms.push(ion(symbol, charge));
            let bonds: Vec<_> = (0..6).map(|i| (i, (i + 1) % 6, order.clone())).collect();
            let graph = MoleculeGraph::from_document(&drawing(atoms, &bonds));
            let result = aromaticity::perceive(&graph);
            assert_eq!(result.systems.len(), 1, "{symbol}{charge:+}");
            assert_eq!(
                (result.systems[0].classification, result.systems[0].reason),
                (Classification::Unsupported, Some(reason)),
                "{symbol}{charge:+} with {order:?} bonds"
            );
            assert_eq!(
                identity::canonicalize_chemical(&graph).map(|c| c.code),
                Err(IdentityError::Unsupported(reason)),
                "{symbol}{charge:+} with {order:?} bonds"
            );
        }
    }
}
