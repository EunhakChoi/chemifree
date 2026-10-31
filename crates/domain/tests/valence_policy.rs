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
