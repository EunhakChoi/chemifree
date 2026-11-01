//! Small command-line bridge for the independent public RDKit comparison.

use chemifree_domain::document::{AtomRef, Command, Document, DocumentId, NewAtomPositions, Point};
use chemifree_domain::graph::MoleculeGraph;
use chemifree_domain::smiles::{parse, write, write_canonical};
use chemifree_domain::states::Hydrogens;

fn main() {
    let Some(input) = std::env::args().nth(1) else {
        eprintln!("usage: smiles_reference SMILES");
        std::process::exit(2);
    };
    let graph = match parse(&input) {
        Ok(graph) => graph,
        Err(error) => {
            eprintln!("{}:{}..{}", error.code, error.start, error.end);
            std::process::exit(1);
        }
    };
    for atom in &graph.atoms {
        let symbol = atom.atom.kind.element().expect("SMILES element").symbol();
        let hydrogens = match atom.atom.hydrogens {
            Hydrogens::Inferred => "inferred".to_owned(),
            Hydrogens::Specified(value) => value.to_string(),
            Hydrogens::Unknown => "unknown".to_owned(),
        };
        println!(
            "atom\t{symbol}\t{}\t{}\t{hydrogens}\t{}",
            atom.atom.isotope.unwrap_or(0),
            atom.atom.charge,
            atom.aromatic
        );
    }
    for bond in &graph.bonds {
        println!("bond\t{}\t{}\t{:?}", bond.begin, bond.end, bond.order);
    }
    // The recorded writer's outcome is a row, not an exit status, so a fixture that expects a
    // refusal still gets its reader state compared.
    let order: Vec<_> = (0..graph.atoms.len()).collect();
    match write(&graph, &order) {
        Ok(output) => println!("smiles\t{output}"),
        Err(error) => println!("write_error\t{error:?}"),
    }
    let mut document = Document::new(DocumentId::parse("reference").unwrap(), "Reference").unwrap();
    let mut commands: Vec<_> = graph
        .atoms
        .iter()
        .map(|atom| Command::AddAtom {
            atom: atom.atom.clone(),
            positions: NewAtomPositions::Single(Point::new(0.0, 0.0).unwrap()),
            bond_to: None,
        })
        .collect();
    commands.extend(graph.bonds.iter().map(|bond| Command::AddBond {
        begin: AtomRef::Created(bond.begin),
        end: AtomRef::Created(bond.end),
        order: bond.order.clone(),
    }));
    if let Err(error) = document.apply(&commands) {
        eprintln!("document:{error}");
        std::process::exit(1);
    }
    match write_canonical(&MoleculeGraph::from_document(&document)) {
        Ok(text) => println!("canonical\t{text}"),
        Err(error) => println!("canonical_error\t{error:?}"),
    }
}
