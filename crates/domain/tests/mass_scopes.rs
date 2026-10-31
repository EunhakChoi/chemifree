use std::collections::BTreeMap;

use chemifree_domain::document::{
    Atom, AtomId, AtomRef, BondId, BondOrder, BondTo, Command, Document, DocumentId,
    NewAtomPositions, Point,
};
use chemifree_domain::element::Element;
use chemifree_domain::mass::{FormulaError, FormulaScope, atom_groups, composition};
use chemifree_domain::scene::Selection;
use chemifree_domain::states::{AtomKind, Hydrogens, QueryAtom};

fn atom(n: u32) -> AtomId {
    AtomId::from_serial(n).unwrap()
}
fn new_atom(symbol: &str, hydrogens: Hydrogens) -> Atom {
    Atom {
        hydrogens,
        ..Atom::from(Element::from_symbol(symbol).unwrap())
    }
}
fn add(value: Atom, bonded_to: Option<u32>) -> Command {
    Command::AddAtom {
        atom: value,
        positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
        bond_to: bonded_to.map(|n| BondTo {
            atom: AtomRef::Id(atom(n)),
            order: BondOrder::Single,
        }),
    }
}
fn empty() -> Document {
    Document::new(DocumentId::parse("formula-test").unwrap(), "Formula").unwrap()
}

#[test]
fn selected_bond_endpoints_and_disconnected_components_use_whole_graph_hydrogens() {
    let mut doc = empty();
    let sodium = Atom {
        charge: 1,
        ..new_atom("Na", Hydrogens::Specified(0))
    };
    doc.apply(&[
        add(new_atom("C", Hydrogens::Inferred), None),
        add(new_atom("C", Hydrogens::Inferred), Some(1)),
        add(new_atom("O", Hydrogens::Inferred), Some(2)),
        add(sodium, None),
    ])
    .unwrap();
    let before = doc.clone();
    let h = BTreeMap::from([(atom(1), 3), (atom(2), 2), (atom(3), 1)]);
    let molecule = atom_groups(&doc, FormulaScope::Molecule, None).unwrap();
    assert_eq!(
        composition(&doc, &molecule[0], &h).unwrap().formula(),
        "C2H6NaO+"
    );
    let components = atom_groups(&doc, FormulaScope::Components, None).unwrap();
    assert_eq!(components, [vec![atom(1), atom(2), atom(3)], vec![atom(4)]]);
    assert_eq!(
        composition(&doc, &components[0], &h).unwrap().formula(),
        "C2H6O"
    );
    assert_eq!(
        composition(&doc, &components[1], &h).unwrap().formula(),
        "Na+"
    );

    let mut selected = Selection::default();
    selected.bonds.insert(BondId::from_serial(1).unwrap());
    let group = atom_groups(&doc, FormulaScope::Selection, Some(&selected)).unwrap();
    assert_eq!(composition(&doc, &group[0], &h).unwrap().formula(), "C2H5");
    selected.objects.insert(doc.first_instance());
    let group = atom_groups(&doc, FormulaScope::Selection, Some(&selected)).unwrap();
    assert_eq!(
        composition(&doc, &group[0], &h).unwrap().formula(),
        "C2H6NaO+"
    );
    assert_eq!(
        doc, before,
        "read-only scopes must not alter history or selection"
    );
}

#[test]
fn explicit_hydrogen_vertices_are_counted_once_and_uncertainty_is_refused() {
    let mut doc = empty();
    doc.apply(&[
        add(new_atom("C", Hydrogens::Specified(0)), None),
        add(new_atom("H", Hydrogens::Inferred), Some(1)),
        add(new_atom("H", Hydrogens::Inferred), Some(1)),
        add(new_atom("H", Hydrogens::Inferred), Some(1)),
        add(new_atom("H", Hydrogens::Inferred), Some(1)),
    ])
    .unwrap();
    let all = atom_groups(&doc, FormulaScope::Molecule, None).unwrap();
    assert_eq!(
        composition(&doc, &all[0], &BTreeMap::new())
            .unwrap()
            .formula(),
        "CH4"
    );
    assert_eq!(
        composition(&doc, &[atom(1)], &BTreeMap::new())
            .unwrap()
            .formula(),
        "C"
    );

    let mut query = empty();
    query
        .apply(&[add(Atom::new(AtomKind::Query(QueryAtom::Any)), None)])
        .unwrap();
    assert_eq!(
        composition(&query, &[atom(1)], &BTreeMap::new()),
        Err(FormulaError::QueryAtom(atom(1)))
    );
    let mut unknown = empty();
    unknown
        .apply(&[add(new_atom("C", Hydrogens::Unknown), None)])
        .unwrap();
    assert_eq!(
        composition(&unknown, &[atom(1)], &BTreeMap::new()),
        Err(FormulaError::UnknownHydrogens(atom(1)))
    );
    let mut selected = Selection::default();
    selected.atoms.insert(atom(99));
    assert_eq!(
        atom_groups(&doc, FormulaScope::Selection, Some(&selected)),
        Err(FormulaError::InvalidSelection)
    );
}
